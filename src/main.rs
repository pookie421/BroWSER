use browser_engine::{html, png_image, render, text};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MAX_INPUT: u64 = html::MAX_INPUT_BYTES as u64;
const USAGE: &str = "BroWSER offline fixture renderer
Usage:
  browser-engine inspect --fixture PATH --output DOM.json [--fixture-root DIR]
      [--parse-mode document | --fragment-context div]
  browser-engine style --fixture PATH --output STYLE.json [--fixture-root DIR]
      [--user-css CSS]
  browser-engine shape --fixture PATH --output GLYPHS.json [--fixture-root DIR]
      --script Latn|Arab|Deva --direction ltr|rtl --language TAG [--scale 1024]
  browser-engine decode-png --fixture PATH --output PNG.json [--fixture-root DIR]
  browser-engine render --fixture PATH --output IMAGE.ppm [--layout LAYOUT.json]
      [--width 640] [--height 480] [--fixture-root DIR] [--resource NAME=PATH]...
      [--profile bootstrap|m1]

The fixture root defaults to fixtures/m1. Input must resolve inside it.
Inspect defaults to strict parsing. The document and div-fragment modes support
bounded HTML tree construction for controlled fixtures.

Outputs must not exist. Parent directories must exist.
This unsandboxed developer tool accepts controlled synthetic fixtures only.
See docs/M1_SCOPE.md for the supported fixture grammar and remaining gates.";

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
    if !matches!(
        command,
        "inspect" | "style" | "shape" | "decode-png" | "render"
    ) {
        return Err(format!("unknown command {command:?}; use --help"));
    }
    let mut flags = BTreeMap::new();
    let mut resource_specs = Vec::new();
    for pair in args[1..].chunks(2) {
        if pair.len() != 2 || !pair[0].starts_with("--") {
            return Err("each option requires a value; use --help".into());
        }
        let name = pair[0].as_str();
        if command == "render" && name == "--resource" {
            resource_specs.push(
                pair[1]
                    .split_once('=')
                    .ok_or("--resource requires NAME=PATH")?,
            );
            continue;
        }
        let allowed = matches!(name, "--fixture" | "--output" | "--fixture-root")
            || match command {
                "inspect" => matches!(name, "--parse-mode" | "--fragment-context"),
                "style" => name == "--user-css",
                "shape" => matches!(name, "--script" | "--direction" | "--language" | "--scale"),
                "render" => matches!(name, "--layout" | "--width" | "--height" | "--profile"),
                _ => false,
            };
        if !allowed {
            return Err(format!("unsupported option {name:?} for {command}"));
        }
        if flags.insert(name, pair[1].as_str()).is_some() {
            return Err(format!("duplicate option {name:?}"));
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
    let limit = if command == "decode-png" {
        png_image::MAX_ENCODED_BYTES as u64
    } else {
        MAX_INPUT
    };
    let (input_path, bytes) = read_fixture(input, &fixture_root, limit)?;
    if command == "decode-png" {
        let (width, height) = png_image::PngImage::decode(&bytes)?.size();
        let json = format!("{{\"width\":{width},\"height\":{height}}}\n");
        return write_outputs(
            &input_path,
            vec![(PathBuf::from(output), json.into_bytes())],
        );
    }
    let input_text = std::str::from_utf8(&bytes).map_err(|_| "fixture must be valid UTF-8")?;
    let json = match command {
        "inspect" => {
            let document = match (flags.get("--parse-mode"), flags.get("--fragment-context")) {
                (None, None) => html::parse(input_text)?,
                (Some(_), Some(_)) => {
                    return Err("choose one of --parse-mode and --fragment-context".into());
                }
                (Some(&"document"), None) => html::parse_document(input_text)?,
                (Some(_), None) => return Err("--parse-mode must be document".into()),
                (None, Some(context)) => html::parse_fragment(input_text, context)?,
            };
            let json = (document.canonical_json() + "\n").into_bytes();
            write_outputs(&input_path, vec![(PathBuf::from(output), json)])?;
            println!(
                "{{\"command\":\"inspect\",\"nodes\":{}}}",
                document.nodes().len()
            );
            return Ok(());
        }
        "style" => {
            let document = html::parse_document(input_text)?;
            render::inspect_styles(&document, flags.get("--user-css").copied().unwrap_or(""))?
        }
        "shape" => {
            let font =
                text::PinnedFont::for_script(flags.get("--script").ok_or("--script is required")?)?;
            let direction = flags
                .get("--direction")
                .ok_or("--direction is required")?
                .parse()?;
            let language = flags.get("--language").ok_or("--language is required")?;
            let scale = flags
                .get("--scale")
                .unwrap_or(&"1024")
                .parse::<u32>()
                .map_err(|_| "--scale must be a positive integer")?;
            font.shape(input_text, direction, language, scale)?
                .canonical_json()
        }
        "render" => {
            let pinned_text = match flags.get("--profile") {
                None | Some(&"bootstrap") => false,
                Some(&"m1") => true,
                Some(_) => return Err("--profile must be bootstrap or m1".to_owned()),
            };
            let document = if pinned_text {
                html::parse_document(input_text)?
            } else {
                html::parse(input_text)?
            };
            let dimension = |name, fallback| -> Result<u32, String> {
                flags
                    .get(name)
                    .copied()
                    .unwrap_or(fallback)
                    .parse::<u32>()
                    .ok()
                    .filter(|value| (1..=2048).contains(value))
                    .ok_or_else(|| format!("{name} must be an integer in 1..=2048"))
            };
            let width = dimension("--width", "640")?;
            let height = dimension("--height", "480")?;
            let mut resources = png_image::PngResources::default();
            for (name, path) in resource_specs {
                let (_, encoded) =
                    read_fixture(path, &fixture_root, png_image::MAX_ENCODED_BYTES as u64)?;
                resources.insert(name, &encoded)?;
            }
            let rendered = if pinned_text {
                render::render_m1(&document, width, height, &resources)?
            } else {
                render::render_with_resources(&document, width, height, &resources)?
            };
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
                document.nodes().len()
            );
            return Ok(());
        }
        _ => unreachable!("validated command"),
    };
    write_outputs(
        &input_path,
        vec![(PathBuf::from(output), (json + "\n").into_bytes())],
    )
}

fn read_fixture(path: &str, fixture_root: &Path, limit: u64) -> Result<(PathBuf, Vec<u8>), String> {
    let input_path = Path::new(path)
        .canonicalize()
        .map_err(|e| format!("cannot resolve fixture: {e}"))?;
    if !input_path.starts_with(fixture_root) || !input_path.is_file() {
        return Err("fixture must be a regular file inside --fixture-root".into());
    }
    let file = File::open(&input_path).map_err(|e| format!("cannot open fixture: {e}"))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read fixture: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("fixture exceeds the {limit}-byte input limit"));
    }
    Ok((input_path, bytes))
}

/// Stage output bytes next to their destination, then publish without overwriting.
/// A failed publication rolls back files created by this invocation. A caller
/// watching both paths can observe the first link before publication completes.
fn write_outputs(input: &Path, outputs: Vec<(PathBuf, Vec<u8>)>) -> Result<(), String> {
    write_outputs_with(
        input,
        outputs,
        |from, to| fs::hard_link(from, to),
        |path| fs::remove_file(path),
    )
}

fn write_outputs_with(
    input: &Path,
    outputs: Vec<(PathBuf, Vec<u8>)>,
    link: impl Fn(&Path, &Path) -> io::Result<()>,
    remove: impl Fn(&Path) -> io::Result<()>,
) -> Result<(), String> {
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
        let exists = match fs::symlink_metadata(&destination) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => {
                return Err(format!(
                    "cannot check output {}: {error}",
                    destination.display()
                ));
            }
        };
        if destination == input || exists {
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
                .map_err(|e| {
                    format!("cannot write staged output {}: {e}", destination.display())
                })?;
        }
        for (temporary, destination) in &staged {
            link(temporary, destination)
                .map_err(|e| format!("cannot publish output {}: {e}", destination.display()))?;
            published.push(destination.clone());
        }
        Ok(())
    })();
    let mut errors = result.err().into_iter().collect::<Vec<_>>();
    for (temporary, _) in &staged {
        if let Err(error) = remove(temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            errors.push(format!(
                "cannot remove staged output {}: {error}",
                temporary.display()
            ));
        }
    }
    if errors.is_empty() {
        return Ok(());
    }
    for path in published {
        if let Err(error) = remove(&path)
            && error.kind() != io::ErrorKind::NotFound
        {
            errors.push(format!(
                "cannot roll back published output {}: {error}",
                path.display()
            ));
        }
    }
    Err(errors.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_failure_preserves_the_collision_and_reports_rollback_errors() {
        for fail_rollback in [false, true] {
            let directory = std::env::temp_dir().join(format!(
                "browser-publish-{}-{fail_rollback}",
                std::process::id()
            ));
            fs::create_dir(&directory).unwrap();
            let directory = directory.canonicalize().unwrap();
            let input = directory.join("fixture.html");
            let image = directory.join("image.ppm");
            let layout = directory.join("layout.json");
            fs::write(&input, b"<div></div>").unwrap();
            let error = write_outputs_with(
                &input,
                vec![(image.clone(), vec![1]), (layout.clone(), vec![2])],
                |from, to| {
                    if to == layout {
                        assert_eq!(fs::read(&image).unwrap(), [1]);
                        File::create_new(to)?.write_all(b"competing output")?;
                    }
                    fs::hard_link(from, to)
                },
                |path| {
                    if fail_rollback && path == image {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "injected denial",
                        ));
                    }
                    fs::remove_file(path)
                },
            )
            .unwrap_err();
            assert!(error.contains(&format!("cannot publish output {}", layout.display())));
            assert_eq!(image.exists(), fail_rollback);
            if fail_rollback {
                assert!(error.contains(&format!(
                    "cannot roll back published output {}: injected denial",
                    image.display()
                )));
            }
            assert_eq!(fs::read(&input).unwrap(), b"<div></div>");
            assert_eq!(fs::read(&layout).unwrap(), b"competing output");
            assert_eq!(
                fs::read_dir(&directory).unwrap().count(),
                2 + usize::from(fail_rollback)
            );
            fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn staged_file_cleanup_failure_rolls_back_a_successful_link() {
        let directory =
            std::env::temp_dir().join(format!("browser-stage-cleanup-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        let input = directory.join("fixture.html");
        let destination = directory.join("output.json");
        fs::write(&input, b"<div></div>").unwrap();
        let stage = directory.join(format!(".browser-stage-{}-0", std::process::id()));
        let error = write_outputs_with(
            &input,
            vec![(destination.clone(), vec![7])],
            |from, to| fs::hard_link(from, to),
            |path| {
                if path == stage {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected stage cleanup denial",
                    ));
                }
                fs::remove_file(path)
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            format!(
                "cannot remove staged output {}: injected stage cleanup denial",
                stage.display()
            )
        );
        assert_eq!(fs::read(&stage).unwrap(), [7]);
        assert!(!destination.exists());
        assert_eq!(fs::read(&input).unwrap(), b"<div></div>");
        fs::remove_dir_all(directory).unwrap();
    }
}
