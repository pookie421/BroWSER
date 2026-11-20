use browser_engine::text::{Direction, MAX_FONT_SCALE, MAX_TEXT_BYTES, PinnedFont, ShapedRun};

const SCALE: u32 = 1024;

fn glyph_json(json: &str) -> String {
    json.rsplit_once("\"glyphs\":")
        .unwrap()
        .1
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect()
}

#[test]
fn pinned_fonts_match_independent_opentype_references() {
    let cases = [
        (
            PinnedFont::Latin,
            Direction::Ltr,
            "office",
            include_str!("../fixtures/m1/expected/shape-latin.json"),
        ),
        (
            PinnedFont::Arabic,
            Direction::Rtl,
            "سلام",
            include_str!("../fixtures/m1/expected/shape-arabic.json"),
        ),
        (
            PinnedFont::Devanagari,
            Direction::Ltr,
            "नमस्ते",
            include_str!("../fixtures/m1/expected/shape-devanagari.json"),
        ),
    ];
    for (font, direction, text, reference) in cases {
        let run = font
            .shape(text, direction, font.default_language(), SCALE)
            .unwrap();
        assert_eq!(font, run.font);
        assert_eq!(text, run.text);
        assert_eq!(direction, run.direction);
        assert_eq!(SCALE, run.scale);
        assert_eq!(glyph_json(&run.canonical_json()), glyph_json(reference));

        let metrics = font.metrics(SCALE).unwrap();
        assert!(metrics.ascent > 0 && metrics.descent > 0);
        // Exercise every reference glyph at a fractional origin. An actual ink
        // mask must contain nonzero and antialiased coverage for this text.
        for glyph in run.glyphs {
            let mask = font.rasterize_glyph(glyph.id, SCALE, 21, 43).unwrap();
            assert_eq!(mask.coverage.len(), (mask.width * mask.height) as usize);
            assert!(mask.coverage.iter().any(|alpha| (1..255).contains(alpha)));
            assert_eq!(mask, font.rasterize_glyph(glyph.id, SCALE, 21, 43).unwrap());
        }
    }
}

#[test]
fn raster_keeps_empty_glyphs_and_rejects_invalid_ids_and_phases() {
    let font = PinnedFont::Latin;
    let space = font.shape(" ", Direction::Ltr, "en", SCALE).unwrap();
    assert!(space.advance() > 0);
    let id = space.glyphs[0].id;
    let mask = font.rasterize_glyph(id, SCALE, 0, 0).unwrap();
    assert_eq!((0, 0), (mask.width, mask.height));
    assert!(mask.coverage.is_empty());
    assert!(font.rasterize_glyph(0, SCALE, 0, 0).is_err());
    assert!(font.rasterize_glyph(u32::MAX, SCALE, 0, 0).is_err());
    assert!(font.rasterize_glyph(id, SCALE, 64, 0).is_err());
    assert!(font.rasterize_glyph(id, SCALE, 0, 64).is_err());
}

#[test]
fn text_limits_report_failures_without_fallback_faces() {
    let font = PinnedFont::Latin;
    let largest = "x".repeat(MAX_TEXT_BYTES);
    assert_eq!(
        font.shape(&largest, Direction::Ltr, "en", SCALE)
            .unwrap()
            .glyphs
            .len(),
        MAX_TEXT_BYTES
    );
    assert!(
        font.shape(&(largest + "x"), Direction::Ltr, "en", SCALE)
            .unwrap_err()
            .contains("bytes")
    );
    assert!(
        font.shape("🦀", Direction::Ltr, "en", SCALE)
            .unwrap_err()
            .contains("no glyph for text at byte 0")
    );
    for scale in [0, MAX_FONT_SCALE + 1] {
        assert!(font.shape("a", Direction::Ltr, "en", scale).is_err());
    }
    for language in ["", "en--US", "en_US", "ع", "abcdefghijk"] {
        assert!(font.shape("a", Direction::Ltr, language, SCALE).is_err());
    }
    assert!(font.shape("a\nb", Direction::Ltr, "en", SCALE).is_err());
    assert!(PinnedFont::for_family("sans-serif").is_err());
    assert!(PinnedFont::for_script("Cyrl").is_err());
    assert!("vertical".parse::<Direction>().is_err());
}

#[test]
fn wraps_actual_shape_advances_and_collapses_only_html_whitespace() {
    let font = PinnedFont::Latin;
    let measure = |text| font.shape(text, Direction::Ltr, "en", SCALE).unwrap();
    let width = measure("one two").advance();
    let input = "   one\t \n two \r\n  three \x0c";
    let wrapped = font
        .wrap(input, Direction::Ltr, "en", SCALE, width)
        .unwrap();
    assert_eq!("one two three", wrapped.collapsed_text);
    assert_eq!(vec![measure("one two"), measure("three")], wrapped.lines);

    let wrapped = font
        .wrap("one\u{a0}two next", Direction::Ltr, "en", SCALE, 0)
        .unwrap();
    let texts = wrapped
        .lines
        .iter()
        .map(|line: &ShapedRun| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(vec!["one\u{a0}two", "next"], texts);
    assert_eq!("one\u{a0}two next", wrapped.collapsed_text);

    let whitespace_only = font
        .wrap(" \t \r\n\x0c", Direction::Ltr, "en", SCALE, width)
        .unwrap();
    assert!(whitespace_only.collapsed_text.is_empty());
    assert!(whitespace_only.lines.is_empty());
}

#[test]
fn wrap_honors_rtl_line_order_and_limits_repeat_shaping_work() {
    let font = PinnedFont::Arabic;
    let first = font.shape("سلام", Direction::Rtl, "ar", SCALE).unwrap();
    let wrapped = font
        .wrap("سلام سلام", Direction::Rtl, "ar", SCALE, first.advance())
        .unwrap();
    assert_eq!(vec![first.clone(), first], wrapped.lines);
    let error = PinnedFont::Latin
        .wrap(&"a ".repeat(8_192), Direction::Ltr, "en", SCALE, i64::MAX)
        .unwrap_err();
    assert!(error.contains("line wrapping exceeds"));
}
