use browser_engine::{html, render};
use std::collections::BTreeSet;

#[test]
fn leading_utf8_signature_preserves_render_artifacts() {
    let source = "<!doctype html><div style='width:2px;height:2px;background:red'></div>";
    let plain = render::render(&html::parse(source).unwrap(), 4, 4).unwrap();
    let with_signature =
        render::render(&html::parse(&format!("\u{feff}{source}")).unwrap(), 4, 4).unwrap();
    assert_eq!(plain.layout_json, with_signature.layout_json);
    assert_eq!(plain.ppm, with_signature.ppm);
}

/// Exercise every seed with deletions, insertions, and replacements at Unicode
/// scalar boundaries. This smoke test is separate from sustained fuzzing.
#[test]
fn controlled_mutations_never_panic_and_successful_renders_are_stable() {
    let seeds = [
        "<div style='width:8px;height:8px;background:red'></div>",
        "<style>div > .a{padding:2px;border:1px solid blue}</style><div><div class=a></div></div>",
        "<div a='&amp;&#128;'><!-- comment --><script><x></script></div>",
        "\u{feff}<!doctype html><title>العربية &amp; नमस्ते</title><div data-label='café🙂'></div>",
    ];
    let edit_chars = [
        '<', '>', '/', '\'', '&', 'x', '0', ' ', '\0', '\r', 'é', 'म', '🙂',
    ];
    let mut attempts = 0;
    let mut parse_successes = 0;
    let mut render_successes = 0;
    for (index, seed) in seeds.into_iter().enumerate() {
        let mut deletions = BTreeSet::new();
        let mut insertions = BTreeSet::new();
        let mut replacements = BTreeSet::new();
        for (at, original) in seed.char_indices() {
            let before = &seed[..at];
            let after = &seed[at + original.len_utf8()..];
            deletions.insert(format!("{before}{after}"));
            for replacement in edit_chars {
                insertions.insert(format!("{before}{replacement}{original}{after}"));
                if replacement != original {
                    replacements.insert(format!("{before}{replacement}{after}"));
                }
            }
        }
        insertions.extend(edit_chars.map(|character| format!("{seed}{character}")));

        for (edit, inputs) in [
            ("deletion", deletions),
            ("insertion", insertions),
            ("replacement", replacements),
        ] {
            attempts += inputs.len();
            let mut parsed = 0;
            let mut rendered = 0;
            for input in inputs {
                let result = std::panic::catch_unwind(|| replay_mutation(&input));
                let (parse_ok, render_ok) = result.unwrap_or_else(|_| {
                    panic!("seed {} {edit} failed on {input:?}", index + 1);
                });
                parsed += usize::from(parse_ok);
                rendered += usize::from(render_ok);
            }
            assert!(
                parsed >= 10 && rendered >= 10,
                "seed {} {edit} reached only {parsed} parse and {rendered} render comparisons",
                index + 1
            );
            parse_successes += parsed;
            render_successes += rendered;
        }
    }
    eprintln!(
        "{attempts} mutations, {parse_successes} successful parses, {render_successes} successful render comparisons"
    );
}

fn replay_mutation(input: &str) -> (bool, bool) {
    let parsed = html::parse(input);
    assert_eq!(parsed, html::parse(input));
    let Ok(document) = parsed else {
        return (false, false);
    };
    let first = render::render(&document, 32, 32);
    let second = render::render(&document, 32, 32);
    match (first, second) {
        (Ok(first), Ok(second)) => {
            assert_eq!(first.ppm, second.ppm);
            assert_eq!(first.layout_json, second.layout_json);
            (true, true)
        }
        (Err(first), Err(second)) => {
            assert_eq!(first, second);
            (true, false)
        }
        _ => panic!("render outcome changed during replay"),
    }
}
