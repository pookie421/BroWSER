use browser_engine::{html, render};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MAX_INPUT: u64 = 1_048_576;
const USAGE: &str = "BroWSER offline fixture renderer\n\
Usage:\n\
  browser-engine inspect --fixture PATH --output DOM.json [--fixture-root DIR]\n\
  browser-engine render --fixture PATH --output IMAGE.ppm [--layout LAYOUT.json]\n\
      [--width 640] [--height 480] [--fixture-root DIR]\n\n\
The fixture root defaults to fixtures/m1. Input must resolve inside it.\n\
Outputs must not exist. Parent directories must exist.\n\
This unsandboxed developer tool accepts controlled synthetic fixtures only.\n\
Full HTML/CSS recovery, visible text, image resources and network browsing are\n\
outside the bootstrap renderer. See docs/M1_SCOPE.md for scope and M1 gates.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("browser-engine: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string()
                .map_err(|_| "arguments must be UTF-8".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        println!("{USAGE}");
        return Ok(());
    }
    if args == ["--version"] {
        println!(
            "browser-engine {} (M1 bootstrap)",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    let command = args[0].as_str();
    if !matches!(command, "inspect" | "render") {
        return Err(format!("unknown command {command:?}; use --help"));
    }
    let mut flags = BTreeMap::new();
    for pair in args[1..].chunks(2) {
        if pair.len() != 2 || !pair[0].starts_with("--") {
            return Err("each option requires a value; use --help".into());
        }
        let allowed = match command {
            "inspect" => matches!(
                pair[0].as_str(),
                "--fixture" | "--output" | "--fixture-root"
            ),
            _ => matches!(
                pair[0].as_str(),
                "--fixture" | "--output" | "--fixture-root" | "--layout" | "--width" | "--height"
            ),
        };
        if !allowed {
            return Err(format!("unsupported option {:?} for {command}", pair[0]));
        }
        if flags.insert(pair[0].as_str(), pair[1].as_str()).is_some() {
            return Err(format!("duplicate option {:?}", pair[0]));
        }
    }
    let input = flags.get("--fixture").ok_or("--fixture is required")?;
    let output = flags.get("--output").ok_or("--output is required")?;
    let fixture_root = Path::new(
        flags
            .get("--fixture-root")
            .copied()
            .unwrap_or("fixtures/m1"),
    )
    .canonicalize()
    .map_err(|e| format!("cannot resolve fixture root: {e}"))?;
    let input_path = Path::new(input)
        .canonicalize()
        .map_err(|e| format!("cannot resolve fixture: {e}"))?;
    if !input_path.starts_with(&fixture_root) || !input_path.is_file() {
        return Err("fixture must be a regular file inside --fixture-root".into());
    }
    let file = File::open(&input_path).map_err(|e| format!("cannot open fixture: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read fixture: {e}"))?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(format!("fixture exceeds the {MAX_INPUT}-byte input limit"));
    }
    let input_text = std::str::from_utf8(&bytes).map_err(|_| "fixture must be valid UTF-8")?;
    let document = html::parse(input_text)?;
    if command == "inspect" {
        write_outputs(
            &input_path,
            vec![(
                PathBuf::from(output),
                (document.canonical_json() + "\n").into_bytes(),
            )],
        )?;
        println!(
            "{{\"command\":\"inspect\",\"nodes\":{}}}",
            document.nodes.len()
        );
    } else {
        let dimension = |name, fallback| -> Result<u32, String> {
            let value = flags.get(name).copied().unwrap_or(fallback);
            let parsed = value
                .parse::<u32>()
                .map_err(|_| format!("{name} must be an integer in 1..=2048"))?;
            if !(1..=2048).contains(&parsed) {
                return Err(format!("{name} must be an integer in 1..=2048"));
            }
            Ok(parsed)
        };
        let width = dimension("--width", "640")?;
        let height = dimension("--height", "480")?;
        let rendered = render::render(&document, width, height)?;
        let image_bytes = rendered.ppm.len();
        let mut outputs = vec![(PathBuf::from(output), rendered.ppm)];
        if let Some(layout) = flags.get("--layout") {
            outputs.push((
                PathBuf::from(layout),
                (rendered.layout_json + "\n").into_bytes(),
            ));
        }
        write_outputs(&input_path, outputs)?;
        println!(
            "{{\"command\":\"render\",\"nodes\":{},\"width\":{width},\"height\":{height},\"image_bytes\":{image_bytes}}}",
            document.nodes.len()
        );
    }
    Ok(())
}

/// Stage output bytes next to their destination, then publish without overwriting.
/// Failed writes/links remove only files created by this invocation.
fn write_outputs(input: &Path, outputs: Vec<(PathBuf, Vec<u8>)>) -> Result<(), String> {
    let mut prepared = Vec::new();
    for (path, bytes) in outputs {
        let name = path.file_name().ok_or("output must name a file")?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let destination = parent
            .canonicalize()
            .map_err(|e| format!("cannot resolve output directory: {e}"))?
            .join(name);
        if destination == input || fs::symlink_metadata(&destination).is_ok() {
            return Err(format!(
                "output already exists or aliases the fixture: {}",
                destination.display()
            ));
        }
        if prepared
            .iter()
            .any(|(p, _): &(PathBuf, Vec<u8>)| *p == destination)
        {
            return Err("output and layout destinations must differ".into());
        }
        prepared.push((destination, bytes));
    }
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut published: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<(), String> {
        for (index, (destination, bytes)) in prepared.iter().enumerate() {
            let temporary = destination
                .with_file_name(format!(".browser-stage-{}-{index}", std::process::id()));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| format!("cannot stage output {}: {e}", destination.display()))?;
            staged.push((temporary, destination.clone()));
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| format!("cannot write output: {e}"))?;
        }
        for (temporary, destination) in &staged {
            fs::hard_link(temporary, destination)
                .map_err(|e| format!("cannot publish output {}: {e}", destination.display()))?;
            published.push(destination.clone());
        }
        Ok(())
    })();
    for (temporary, _) in &staged {
        let _ = fs::remove_file(temporary);
    }
    if result.is_err() {
        for path in published {
            let _ = fs::remove_file(path);
        }
    }
    result
}
