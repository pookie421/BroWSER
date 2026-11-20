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
    let pixels = ppm.strip_prefix(b"P6\n8 8\n255\n").unwrap();
    assert_eq!(pixels.len(), 8 * 8 * 3);
    let pixel = |x: usize, y: usize| {
        let start = (y * 8 + x) * 3;
        &pixels[start..start + 3]
    };
    assert_eq!(pixel(3, 2), [255, 0, 0]);
    assert_eq!(pixel(4, 2), [255, 255, 255]);
    assert_eq!(
        fs::read_to_string(fixture.0.join("layout.json")).unwrap(),
        concat!(
            r#"{"viewport":{"width":8,"height":8},"boxes":[{"node":1,"tag":"div","#,
            r#""border_box":{"x":0,"y":0,"width":4,"height":3},"#,
            r#""content_box":{"x":0,"y":0,"width":4,"height":3},"#,
            r##""clip":{"x":0,"y":0,"width":8,"height":8},"background":"#ff0000"}]}"##,
            "\n\n",
        ),
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
fn inspection_modes_are_explicit_and_exclusive() {
    let fixture = Fixture::new(b"<p>one<p>two");
    let mut args = vec![
        "inspect",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "dom.json",
    ];
    let dom = fixture.0.join("dom.json");
    assert!(!fixture.run(&args).status.success());
    assert!(!dom.exists());

    args.extend(["--parse-mode", "document", "--fragment-context", "div"]);
    let conflict = fixture.run(&args);
    assert_eq!(
        String::from_utf8(conflict.stderr).unwrap(),
        "browser-engine: choose one of --parse-mode and --fragment-context\n"
    );
    assert!(!dom.exists());

    args.truncate(args.len() - 2);
    let result = fixture.run(&args);
    assert!(result.status.success(), "{:?}", result.stderr);
    let document = fs::read_to_string(&dom).unwrap();
    for name in ["html", "head", "body"] {
        assert!(document.contains(&format!("\"name\":\"{name}\"")));
    }
    assert_eq!(document.matches("\"name\":\"p\"").count(), 2);
    fs::remove_file(&dom).unwrap();

    args.truncate(args.len() - 2);
    args.extend(["--fragment-context", "div"]);
    let result = fixture.run(&args);
    assert!(result.status.success(), "{:?}", result.stderr);
    let fragment = fs::read_to_string(&dom).unwrap();
    assert!(!fragment.contains("\"name\":\"html\""));
    assert_eq!(fragment.matches("\"name\":\"p\"").count(), 2);
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
fn oversized_input_fails_without_output() {
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
    assert!(!fixture.0.join("dom.json").exists());
}

#[test]
fn viewport_bounds_apply_to_both_dimensions() {
    let fixture = Fixture::new(b"<div></div>");
    let image = fixture.0.join("image.ppm");
    let layout = fixture.0.join("layout.json");
    for varied in ["--width", "--height"] {
        for (value, accepted) in [
            ("0", false),
            ("1", true),
            ("2047", true),
            ("2048", true),
            ("2049", false),
        ] {
            let (width, height) = if varied == "--width" {
                (value, "1")
            } else {
                ("1", value)
            };
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
                "--width",
                width,
                "--height",
                height,
            ]);
            assert_eq!(result.status.success(), accepted, "{varied} {value}");
            if accepted {
                assert!(
                    fs::read(&image)
                        .unwrap()
                        .starts_with(format!("P6\n{width} {height}\n255\n").as_bytes())
                );
                assert!(layout.is_file());
                fs::remove_file(&image).unwrap();
                fs::remove_file(&layout).unwrap();
            } else {
                assert_eq!(
                    String::from_utf8(result.stderr).unwrap(),
                    format!("browser-engine: {varied} must be an integer in 1..=2048\n")
                );
                assert!(!image.exists());
                assert!(!layout.exists());
            }
        }
    }
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

#[test]
fn unsupported_paint_property_cannot_publish_an_incorrect_image() {
    let fixture =
        Fixture::new(b"<div style='width:2px;height:2px;background:red;opacity:0'></div>");
    let args = [
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
        "4",
        "--height",
        "4",
    ];
    let result = fixture.run(&args);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(result.stderr).unwrap(),
        "browser-engine: unsupported bootstrap CSS property: opacity\n"
    );
    assert!(!fixture.0.join("image.ppm").exists());
    assert!(!fixture.0.join("layout.json").exists());

    fs::write(
        fixture.0.join("input.html"),
        b"<div style='width:2px;height:2px;background:red'></div>",
    )
    .unwrap();
    assert!(fixture.run(&args).status.success());
    let image = fs::read(fixture.0.join("image.ppm")).unwrap();
    assert_eq!(
        &image.strip_prefix(b"P6\n4 4\n255\n").unwrap()[..3],
        [255, 0, 0]
    );
}

#[test]
fn png_render_uses_only_declared_fixture_resources() {
    let fixture = Fixture::new(b"<div><img src='checker.png'></div>");
    let source = include_bytes!("../fixtures/m1/assets/checker.png");
    fs::write(fixture.0.join("checker.png"), source).unwrap();
    let args = [
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
        "2",
        "--height",
        "2",
    ];
    let missing = fixture.run(&args);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("requires a declared PNG resource"));
    assert!(!fixture.0.join("image.ppm").exists());
    assert!(!fixture.0.join("layout.json").exists());

    let outside = Fixture::new(source);
    let escaped = format!("checker.png={}", outside.0.join("input.html").display());
    let escaped = fixture.run(&[args.as_slice(), &["--resource", &escaped]].concat());
    assert_eq!(escaped.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&escaped.stderr).contains("inside --fixture-root"));
    assert!(!fixture.0.join("image.ppm").exists());
    assert!(!fixture.0.join("layout.json").exists());

    let rendered =
        fixture.run(&[args.as_slice(), &["--resource", "checker.png=checker.png"]].concat());
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let ppm = fs::read(fixture.0.join("image.ppm")).unwrap();
    assert_eq!(
        ppm.strip_prefix(b"P6\n2 2\n255\n").unwrap(),
        [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]
    );
    assert!(fixture.0.join("layout.json").is_file());
}

#[test]
fn png_decode_reports_dimensions_and_rejects_truncation_without_output() {
    let fixture = Fixture::new(include_bytes!("../fixtures/m1/assets/truncated.png"));
    let args = [
        "decode-png",
        "--fixture-root",
        ".",
        "--fixture",
        "input.html",
        "--output",
        "png.json",
    ];
    let rejected = fixture.run(&args);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&rejected.stderr)
            .starts_with("browser-engine: invalid PNG header:")
    );
    assert!(!fixture.0.join("png.json").exists());

    fs::write(
        fixture.0.join("input.html"),
        include_bytes!("../fixtures/m1/assets/checker.png"),
    )
    .unwrap();
    assert!(fixture.run(&args).status.success());
    assert_eq!(
        fs::read(fixture.0.join("png.json")).unwrap(),
        b"{\"width\":2,\"height\":2}\n"
    );
}
