#![no_main]

use browser_engine::{html, png_image::PngResources, render};
use libfuzzer_sys::{Corpus, fuzz_target};

fuzz_target!(|bytes: &[u8]| -> Corpus {
    let [profile, width, height, source @ ..] = bytes else {
        return Corpus::Reject;
    };
    let Ok(source) = std::str::from_utf8(source) else {
        return Corpus::Reject;
    };
    // Restrict viewports to small surfaces so parse/layout/paint can be
    // explored in one process. A zero dimension still tests input rejection.
    let width = u32::from(*width % 65);
    let height = u32::from(*height % 65);
    let pinned_text = profile & 1 == 1;
    let parsed = if pinned_text {
        html::parse_document(source)
    } else {
        html::parse(source)
    };
    let Ok(document) = parsed else {
        return Corpus::Keep;
    };
    let resources = PngResources::default();
    let draw = || {
        if pinned_text {
            render::render_m1(&document, width, height, &resources)
        } else {
            render::render(&document, width, height)
        }
    };
    match (draw(), draw()) {
        (Ok(first), Ok(second)) => {
            assert!(width != 0 && height != 0);
            let header = format!("P6\n{width} {height}\n255\n");
            assert!(first.ppm.starts_with(header.as_bytes()));
            assert_eq!(
                first.ppm.len(),
                header.len() + (width * height * 3) as usize
            );
            assert_eq!(first.ppm, second.ppm);
            assert_eq!(first.layout_json, second.layout_json);
        }
        (Err(first), Err(second)) => assert_eq!(first, second),
        _ => panic!("renderer changed outcome on same-process replay"),
    }
    Corpus::Keep
});
