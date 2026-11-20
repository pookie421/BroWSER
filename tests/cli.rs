use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(html: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "browser-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("input.html"), html).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_browser-engine"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inspect_and_render_produce_real_artifacts() {
    let fixture = Fixture::new(b"<div style='width:4px;height:3px;background:red'></div>");
    let inspect = fixture.run(&[
        "inspect",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "dom.json",
    ]);
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    let dom = fs::read_to_string(fixture.0.join("dom.json")).unwrap();
    assert!(dom.contains("\"div\""));
    let render = fixture.run(&[
        "render",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "image.ppm",
        "--layout",
        "layout.json",
        "--width",
        "8",
        "--height",
        "8",
    ]);
    assert!(
        render.status.success(),
        "{}",
        String::from_utf8_lossy(&render.stderr)
    );
    let ppm = fs::read(fixture.0.join("image.ppm")).unwrap();
    assert!(ppm.starts_with(b"P6\n8 8\n255\n"));
    assert_eq!(ppm.len(), b"P6\n8 8\n255\n".len() + 8 * 8 * 3);
    assert!(ppm.windows(3).any(|pixel| pixel == [255, 0, 0]));
    assert!(
        !fs::read_to_string(fixture.0.join("layout.json"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejects_bad_arguments_and_invalid_utf8_without_artifacts() {
    let fixture = Fixture::new(&[0xff]);
    for args in [
        vec![
            "inspect",
            "--fixture-root",
            ".",
            "--fixture",
            "input.html",
            "--output",
            "dom.json",
        ],
        vec!["render", "--width", "0"],
        vec![
            "inspect",
            "--fixture",
            "input.html",
            "--fixture",
            "input.html",
        ],
        vec!["inspect", "--unknown", "value"],
    ] {
        assert!(!fixture.run(&args).status.success());
    }
    assert!(!fixture.0.join("dom.json").exists());
}

#[test]
fn output_collision_preserves_existing_files_and_fixture() {
    let fixture = Fixture::new(b"<div></div>");
    fs::write(fixture.0.join("layout.json"), b"preserve").unwrap();
    let result = fixture.run(&[
        "render",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "image.ppm",
        "--layout",
        "layout.json",
    ]);
    assert!(!result.status.success());
    assert!(!fixture.0.join("image.ppm").exists());
    assert_eq!(
        fs::read(fixture.0.join("layout.json")).unwrap(),
        b"preserve"
    );
    let result = fixture.run(&[
        "inspect",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "input.html",
    ]);
    assert!(!result.status.success());
    assert_eq!(
        fs::read(fixture.0.join("input.html")).unwrap(),
        b"<div></div>"
    );
}

#[test]
fn fixture_root_prevents_escaping_to_other_inputs() {
    let fixture = Fixture::new(b"<div></div>");
    fs::create_dir(fixture.0.join("root")).unwrap();
    let result = fixture.run(&[
        "inspect",
        "--fixture-root",
        "root",
        "--fixture",
        "input.html",
        "--output",
        "dom.json",
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("inside --fixture-root"));
    assert!(!fixture.0.join("dom.json").exists());
}

#[test]
fn input_and_viewport_limits_fail_without_output() {
    let fixture = Fixture::new(&vec![b' '; 1_048_577]);
    let args = [
        "inspect",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "dom.json",
    ];
    let result = fixture.run(&args);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("input limit"));
    fs::write(fixture.0.join("input.html"), b"<div></div>").unwrap();
    let result = fixture.run(&[
        "render",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "image.ppm",
        "--width",
        "2049",
    ]);
    assert!(!result.status.success());
    assert!(!fixture.0.join("image.ppm").exists());
}

#[test]
fn unsupported_text_is_an_error_without_blank_image_success() {
    let fixture = Fixture::new(b"<div>Hello</div>");
    let result = fixture.run(&[
        "render",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "image.ppm",
    ]);
    assert!(!result.status.success());
    assert!(!fixture.0.join("image.ppm").exists());
}
