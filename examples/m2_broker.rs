//! Offline broker experiment using one anonymous pipe pair per synthetic child.

use std::env;
use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const VERSION: u8 = 1;
const READ_RESOURCE: u8 = 1;
const RESPONSE: u8 = 2;
const RESOURCE: [u8; 4] = *b"tile";
const CAPABILITY: u32 = 7;
const INITIAL_GENERATION: u32 = 1;
const REPLAY_TIMEOUT: Duration = Duration::from_secs(5);
const GRANTED: [u8; 9] = [0, 0, 0, 1, 1, b't', b'i', b'l', b'e'];
const WRONG_PEER: [u8; 9] = [0, 0, 0, 1, 0, 0, 0, 0, 0];
const STALE: [u8; 9] = [0, 0, 0, 2, 0, 0, 0, 0, 0];

#[derive(Debug, PartialEq, Eq)]
struct Request {
    id: u32,
    generation: u32,
    capability: u32,
}

impl Request {
    fn read(source: impl Read) -> io::Result<Self> {
        let payload: [u8; 12] = read_frame(source, READ_RESOURCE)?;
        Ok(Self {
            id: u32::from_be_bytes(payload[..4].try_into().expect("four-byte request ID")),
            generation: u32::from_be_bytes(payload[4..8].try_into().expect("four-byte generation")),
            capability: u32::from_be_bytes(
                payload[8..].try_into().expect("four-byte capability ID"),
            ),
        })
    }

    fn write(&self, destination: impl Write) -> io::Result<()> {
        let mut payload = [0; 12];
        payload[..4].copy_from_slice(&self.id.to_be_bytes());
        payload[4..8].copy_from_slice(&self.generation.to_be_bytes());
        payload[8..].copy_from_slice(&self.capability.to_be_bytes());
        write_frame(destination, READ_RESOURCE, payload)
    }
}

fn read_frame<const N: usize>(mut source: impl Read, kind: u8) -> io::Result<[u8; N]> {
    let mut header = [0; 4];
    source.read_exact(&mut header)?;
    let [version, received_kind, length_high, length_low] = header;
    if version != VERSION
        || received_kind != kind
        || usize::from(u16::from_be_bytes([length_high, length_low])) != N
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported broker frame version, type, or length",
        ));
    }
    let mut payload = [0; N];
    source.read_exact(&mut payload)?;
    Ok(payload)
}

fn write_frame<const N: usize>(
    mut destination: impl Write,
    kind: u8,
    payload: [u8; N],
) -> io::Result<()> {
    let length = u16::try_from(N).map_err(io::Error::other)?;
    let [length_high, length_low] = length.to_be_bytes();
    destination.write_all(&[VERSION, kind, length_high, length_low])?;
    destination.write_all(&payload)?;
    destination.flush()
}

struct Broker {
    owner: u32,
    generation: u32,
    grant: Option<u32>,
    resource: [u8; 4],
}

impl Broker {
    fn read_resource(&self, peer: u32, request: &Request) -> Option<[u8; 4]> {
        (peer == self.owner
            && request.generation == self.generation
            && self.grant == Some(request.capability))
        .then_some(self.resource)
    }

    fn advance_document(&mut self, generation: u32) {
        self.generation = generation;
        self.grant = None;
    }
}

fn response(id: u32, resource: Option<[u8; 4]>) -> [u8; 9] {
    let mut payload = [0; 9];
    payload[..4].copy_from_slice(&id.to_be_bytes());
    if let Some(bytes) = resource {
        payload[4] = 1;
        payload[5..].copy_from_slice(&bytes);
    }
    payload
}

struct FixtureChild(Child);

impl FixtureChild {
    fn spawn(command: &mut Command) -> io::Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map(Self)
    }

    fn take_peer(&mut self) -> Peer {
        Peer {
            id: self.0.id(),
            input: self.0.stdin.take().expect("piped child stdin"),
            output: self.0.stdout.take().expect("piped child stdout"),
        }
    }

    fn finish(&mut self, deadline: Instant) -> io::Result<()> {
        loop {
            if let Some(status) = self.0.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!(
                        "synthetic child exited with {status}"
                    )))
                };
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(timeout("waiting for child exit"));
            }
            thread::sleep(remaining.min(Duration::from_millis(5)));
        }
    }

    fn terminate(&mut self) -> io::Result<()> {
        if self.0.try_wait()?.is_none() {
            if let Err(error) = self.0.kill()
                && self.0.try_wait()?.is_none()
            {
                return Err(error);
            }
            self.0.wait()?;
        }
        Ok(())
    }
}

impl Drop for FixtureChild {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

struct Peer {
    id: u32,
    input: ChildStdin,
    output: ChildStdout,
}

impl Peer {
    fn exchange(&mut self, broker: &Broker, expected: Option<[u8; 4]>) -> io::Result<()> {
        let request = Request::read(&mut self.output)?;
        let result = broker.read_resource(self.id, &request);
        if result != expected {
            return Err(io::Error::other("unexpected broker authorization decision"));
        }
        write_frame(&mut self.input, RESPONSE, response(request.id, result))
    }
}

fn child_fixture(requests: &[(u32, [u8; 9])]) -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    for &(id, expected) in requests {
        Request {
            id,
            generation: INITIAL_GENERATION,
            capability: CAPABILITY,
        }
        .write(&mut output)?;
        let received = read_frame(&mut input, RESPONSE)?;
        if received != expected {
            return Err(io::Error::other("broker response differs from fixture"));
        }
    }
    Ok(())
}

fn exchange(mut owner: Peer, mut other: Peer) -> io::Result<()> {
    let mut broker = Broker {
        owner: owner.id,
        generation: INITIAL_GENERATION,
        grant: Some(CAPABILITY),
        resource: RESOURCE,
    };
    other.exchange(&broker, None)?;
    drop(other);
    owner.exchange(&broker, Some(RESOURCE))?;
    broker.advance_document(INITIAL_GENERATION + 1);
    owner.exchange(&broker, None)
}

fn timeout(operation: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!("broker replay timed out {operation}"),
    )
}

fn replay(
    owner: &mut FixtureChild,
    other: &mut FixtureChild,
    duration: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + duration;
    let owner_peer = owner.take_peer();
    let other_peer = other.take_peer();
    let (send, receive) = mpsc::channel();
    // The supervisor retains both process handles. Terminating the fixtures
    // closes their pipe endpoints and releases a worker blocked on I/O.
    let worker = thread::Builder::new().spawn(move || {
        let result = exchange(owner_peer, other_peer);
        let _ = send.send(result);
    })?;
    let result = receive
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => timeout("during child I/O"),
            mpsc::RecvTimeoutError::Disconnected => io::Error::other("broker I/O worker failed"),
        })
        .and_then(|result| result)
        .and_then(|()| other.finish(deadline))
        .and_then(|()| owner.finish(deadline));
    let cleanup_errors: Vec<_> = [("owner", owner.terminate()), ("other", other.terminate())]
        .into_iter()
        .filter_map(|(role, result)| result.err().map(|error| format!("{role} child: {error}")))
        .collect();
    if !cleanup_errors.is_empty() {
        let cleanup = cleanup_errors.join(", ");
        return match result {
            Ok(()) => Err(io::Error::other(format!("cannot reap {cleanup}"))),
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!("{error}\ncannot reap {cleanup}"),
            )),
        };
    }
    worker
        .join()
        .map_err(|_| io::Error::other("broker I/O worker panicked"))?;
    result
}

fn demo() -> io::Result<()> {
    let executable = env::current_exe()?;
    let mut owner = FixtureChild::spawn(Command::new(&executable).arg("--owner-fixture"))?;
    let mut other = FixtureChild::spawn(Command::new(&executable).arg("--other-fixture"))?;
    replay(&mut owner, &mut other, REPLAY_TIMEOUT)?;
    println!(
        "Broker fixture passed: resource granted to owner, wrong peer and stale request denied."
    );
    Ok(())
}

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let mode = args.next();
    let result = match (
        mode.as_deref().and_then(std::ffi::OsStr::to_str),
        args.next(),
    ) {
        (None, None) if mode.is_none() => demo(),
        (Some("--owner-fixture"), None) => child_fixture(&[(1, GRANTED), (2, STALE)]),
        (Some("--other-fixture"), None) => child_fixture(&[(1, WRONG_PEER)]),
        _ => Err(io::Error::other(
            "expected no arguments to run the broker fixture",
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("m2 broker fixture: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: [u8; 16] = [1, 1, 0, 12, 0, 0, 0, 19, 0, 0, 0, 1, 0, 0, 0, 7];

    #[test]
    fn request_matches_the_expected_wire_bytes() {
        let expected = Request {
            id: 19,
            generation: INITIAL_GENERATION,
            capability: CAPABILITY,
        };
        assert_eq!(Request::read(&FRAME[..]).unwrap(), expected);
        let mut encoded = Vec::new();
        expected.write(&mut encoded).unwrap();
        assert_eq!(encoded, FRAME);
    }

    #[test]
    fn response_matches_complete_grant_and_denial_wire_bytes() {
        let mut encoded = Vec::new();
        write_frame(&mut encoded, RESPONSE, response(1, Some(RESOURCE))).unwrap();
        assert_eq!(encoded, [1, 2, 0, 9, 0, 0, 0, 1, 1, b't', b'i', b'l', b'e']);
        encoded.clear();
        write_frame(&mut encoded, RESPONSE, response(2, None)).unwrap();
        assert_eq!(encoded, [1, 2, 0, 9, 0, 0, 0, 2, 0, 0, 0, 0, 0]);
    }

    #[cfg(unix)]
    #[test]
    fn silent_children_are_reaped_after_an_io_deadline() {
        let mut silent = Command::new("/bin/sh");
        silent.args(["-c", "IFS= read -r line"]);
        let mut owner = FixtureChild::spawn(&mut silent).unwrap();
        let mut other = FixtureChild::spawn(&mut silent).unwrap();
        let error = replay(&mut owner, &mut other, Duration::from_millis(100)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(
            error.to_string(),
            "broker replay timed out during child I/O"
        );
        assert!(owner.0.try_wait().unwrap().is_some());
        assert!(other.0.try_wait().unwrap().is_some());
    }

    #[test]
    fn rejects_invalid_headers_before_reading_the_body() {
        for (offset, byte) in [(0, 2), (1, 99), (2, 255), (3, 11), (3, 13)] {
            let mut header = [1, 1, 0, 12];
            header[offset] = byte;
            assert_eq!(
                Request::read(&header[..]).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn rejects_truncated_requests() {
        for length in 0..FRAME.len() {
            assert_eq!(
                Request::read(&FRAME[..length]).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
    }

    #[test]
    fn binds_the_grant_to_its_owner_and_document() {
        let mut broker = Broker {
            owner: 1,
            generation: INITIAL_GENERATION,
            grant: Some(CAPABILITY),
            resource: RESOURCE,
        };
        let mut request = Request::read(&FRAME[..]).unwrap();
        assert_eq!(broker.read_resource(1, &request), Some(RESOURCE));
        assert_eq!(broker.read_resource(2, &request), None);
        request.capability += 1;
        assert_eq!(broker.read_resource(1, &request), None);
        request.capability = CAPABILITY;
        request.generation += 1;
        assert_eq!(broker.read_resource(1, &request), None);
        broker.advance_document(request.generation);
        assert_eq!(broker.read_resource(1, &request), None);
    }
}
