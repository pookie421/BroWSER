use browser_engine::{html, png_image::PngResources, render};

#[test]
fn different_documents_replay_independently_in_one_process() {
    let fixtures = [
        include_str!("../fixtures/m1/full/inline-wrap.html"),
        include_str!("../fixtures/m1/full/png.html"),
        include_str!("../fixtures/m1/full/arabic.html"),
        include_str!("../fixtures/m1/full/devanagari.html"),
    ];
    let draw = |source: &str| {
        let document = html::parse_document(source).unwrap();
        let mut resources = PngResources::default();
        resources
            .insert(
                "checker.png",
                include_bytes!("../fixtures/m1/assets/checker.png"),
            )
            .unwrap();
        render::render_m1(&document, 128, 64, &resources).unwrap()
    };
    let baseline = fixtures.map(draw);
    for _ in 0..3 {
        for (source, previous) in fixtures.iter().zip(&baseline).rev() {
            let output = draw(source);
            assert_eq!(output.layout_json, previous.layout_json);
            assert_eq!(output.ppm, previous.ppm);
        }
    }
}

#[test]
fn mixed_element_and_text_flow_requires_broader_inline_layout() {
    let document = html::parse_document("<p>one <span>two</span></p>").unwrap();
    let error = render::render_m1(&document, 128, 64, &PngResources::default())
        .err()
        .expect("mixed inline flow must produce a diagnostic");
    assert!(error.contains("mixed text and element children"));
}
