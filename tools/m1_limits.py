"""Sample one fixture process's RSS and verify its kernel-reported peak.

Sampling can overshoot between observations. This supervisor measures the
direct child and cleans up its process group. It is not an OS sandbox.
"""

from __future__ import annotations

import ctypes
import errno
import math
import os
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from functools import cache
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import time


RSS_SAMPLE_INTERVAL_SECONDS = 0.010
OUTPUT_LIMIT_BYTES = 1_048_576  # Per stream. Fixture images and JSON go to files.
_REAP_GRACE_SECONDS = 1.0


@dataclass(frozen=True)
class LimitedResult:
    returncode: int
    stdout: bytes
    stderr: bytes
    elapsed_seconds: float
    sampled_peak_rss_bytes: int
    kernel_peak_rss_bytes: int
    termination: str | None
    trigger: str | None

    @property
    def peak_rss_bytes(self) -> int:
        return max(self.kernel_peak_rss_bytes, self.sampled_peak_rss_bytes)


@dataclass(frozen=True)
class _Backend:
    name: str
    sample: Callable[[int], int | None]
    wait4_scale: int


class _DarwinRusageV0(ctypes.Structure):
    # XNU's <sys/resource.h>, RUSAGE_INFO_V0. All sizes here are bytes.
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [
        (name, ctypes.c_uint64)
        for name in (
            "user_time", "system_time", "pkg_idle_wkups", "interrupt_wkups",
            "pageins", "wired_size", "resident_size", "phys_footprint",
            "proc_start_abstime", "proc_exit_abstime",
        )
    ]


def _darwin_sampler() -> Callable[[int], int | None]:
    try:
        query = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True).proc_pid_rusage
    except (OSError, AttributeError) as error:
        raise OSError("fixture RSS needs Darwin libproc") from error
    query.argtypes = (ctypes.c_int, ctypes.c_int, ctypes.POINTER(_DarwinRusageV0))
    query.restype = ctypes.c_int

    def sample(pid: int) -> int | None:
        usage = _DarwinRusageV0()
        ctypes.set_errno(0)
        if query(pid, 0, ctypes.byref(usage)) == 0:
            return usage.resident_size
        error = ctypes.get_errno()
        if error == errno.ESRCH:
            return None
        raise OSError(error, f"libproc could not read fixture RSS: {os.strerror(error)}")

    return sample


def _linux_sample(pid: int) -> int | None:
    # smaps_rollup supplies resident page totals rather than the approximate
    # per-task counters in statm and status.
    path = Path(f"/proc/{pid}/smaps_rollup")
    try:
        content = path.read_bytes()
    except (FileNotFoundError, ProcessLookupError):
        return None
    if not content:
        return None
    for line in content.splitlines():
        if line.startswith(b"Rss:"):
            fields = line.split()
            if len(fields) == 3 and fields[1].isdigit() and fields[2] == b"kB":
                return int(fields[1]) * 1024
            break
    raise OSError("Linux smaps_rollup did not return an RSS reading")


@cache
def _backend() -> _Backend:
    if not hasattr(os, "wait4") or not hasattr(os, "waitid") or not hasattr(os, "WNOWAIT"):
        raise OSError("fixture RSS needs wait4 and waitid(WNOWAIT)")
    if sys.platform == "darwin":
        backend = _Backend("darwin-libproc-wait4", _darwin_sampler(), 1)
    elif sys.platform == "linux":
        backend = _Backend("linux-smaps-rollup-wait4", _linux_sample, 1024)
    else:
        raise OSError(f"fixture RSS sampling is unavailable on {sys.platform}")
    if not backend.sample(os.getpid()):
        raise OSError(f"{backend.name} did not provide positive RSS telemetry")
    return backend


def rss_backend() -> str:
    """Return the available backend name or raise before launching a fixture."""
    return _backend().name


def _exited(pid: int) -> bool:
    # Keep the PID reserved until cleanup has signalled its process group.
    return os.waitid(os.P_PID, pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None


def _reap(process: subprocess.Popen[bytes], *, kill: bool) -> int:
    if kill:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except PermissionError:
            # A child can exit between the terminating sample and the signal.
            # On Darwin a signal to an already-exited group may be denied.
            if not _exited(process.pid):
                raise
    deadline = time.monotonic() + _REAP_GRACE_SECONDS
    while True:
        pid, status, usage = os.wait4(process.pid, os.WNOHANG)
        if pid:
            process.returncode = os.waitstatus_to_exitcode(status)
            return usage.ru_maxrss
        if time.monotonic() >= deadline:
            raise OSError("fixture process did not exit after SIGKILL")
        time.sleep(0.001)


def _capture(selector: selectors.BaseSelector, timeout: float) -> tuple[bool, bool]:
    events = selector.select(timeout)
    for key, _ in events:
        try:
            chunk = os.read(key.fd, 65_536)
        except BlockingIOError:
            continue
        if not chunk:
            selector.unregister(key.fileobj)
            continue
        output = key.data
        remaining = OUTPUT_LIMIT_BYTES - len(output)
        output.extend(chunk[:remaining])
        if len(chunk) > remaining:
            return True, True
    return bool(events), False


def _monitor(
    process: subprocess.Popen[bytes], selector: selectors.BaseSelector, backend: _Backend,
    started: float, timeout: float, rss_limit_bytes: int,
) -> tuple[int, str | None, str | None]:
    next_sample = started
    deadline = started + timeout
    sampled_peak = 0
    while not _exited(process.pid):
        now = time.monotonic()
        if now >= deadline:
            return sampled_peak, "timeout", "wall_time"
        if now >= next_sample:
            sample = backend.sample(process.pid)
            if sample is None:
                if _exited(process.pid):
                    break
                raise OSError(f"{backend.name} lost telemetry for the live fixture")
            sampled_peak = max(sampled_peak, sample)
            if sampled_peak > rss_limit_bytes:
                return sampled_peak, "resource_limit", "sampled_rss"
            next_sample = now + RSS_SAMPLE_INTERVAL_SECONDS
        delay = max(0.0, min(next_sample, deadline) - time.monotonic())
        _, overflow = _capture(selector, delay)
        if overflow:
            return sampled_peak, "resource_limit", "diagnostic_bytes"
    return sampled_peak, None, None


def run_limited(
    command: Sequence[str], *, cwd: Path, timeout: float, rss_limit_bytes: int,
    env: Mapping[str, str] | None = None,
) -> LimitedResult:
    """Run a fixture, sampling its RSS and rejecting a late over-limit peak too."""
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("fixture timeout must be positive and finite")
    if type(rss_limit_bytes) is not int or rss_limit_bytes <= 0:
        raise ValueError("fixture RSS limit must be positive bytes")
    backend = _backend()
    if signal.getsignal(signal.SIGCHLD) == signal.SIG_IGN:
        raise OSError("fixture supervisor needs waitable child processes")
    stdout, stderr = bytearray(), bytearray()
    termination = trigger = None
    finished_monitoring = False
    with selectors.DefaultSelector() as selector:
        started = time.monotonic()
        process = subprocess.Popen(
            command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
        )
        try:
            os.set_blocking(process.stdout.fileno(), False)
            os.set_blocking(process.stderr.fileno(), False)
            selector.register(process.stdout, selectors.EVENT_READ, stdout)
            selector.register(process.stderr, selectors.EVENT_READ, stderr)
            sampled_peak, termination, trigger = _monitor(
                process, selector, backend, started, timeout, rss_limit_bytes,
            )
            finished_monitoring = True
        finally:
            try:
                kernel_peak = _reap(process, kill=not finished_monitoring or termination is not None)
                kernel_peak *= backend.wait4_scale
                # After the child exits, drain its buffered diagnostics without blocking.
                while selector.get_map():
                    ready, overflow = _capture(selector, 0)
                    if overflow and termination is None:
                        termination, trigger = "resource_limit", "diagnostic_bytes"
                    if overflow or not ready:
                        break
            finally:
                process.stdout.close()
                process.stderr.close()
    elapsed = time.monotonic() - started
    if termination is None:
        if kernel_peak <= 0:
            raise OSError("wait4 did not report positive fixture peak RSS")
        if kernel_peak > rss_limit_bytes:
            termination, trigger = "resource_limit", "peak_rss"
        elif elapsed >= timeout:
            termination, trigger = "timeout", "wall_time"
    return LimitedResult(
        process.returncode, bytes(stdout), bytes(stderr), elapsed,
        sampled_peak, kernel_peak, termination, trigger,
    )
