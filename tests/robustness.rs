use browser_engine::{html, render};

/// A reproducible mutation smoke test, separate from the required sustained,
/// coverage-guided fuzz campaign. The mutation index reproduces any panic.
#[test]
fn controlled_mutations_never_panic_and_successful_renders_are_stable() {
    let seeds: [&[u8]; 3] = [
        b"<div style='width:8px;height:8px;background:red'></div>",
        b"<style>div > .a{padding:2px;border:1px solid blue}</style><div><div class=a></div></div>",
        b"<div a='&amp;&#128;'><!-- comment --><script><x></script></div>",
    ];
    let mut state = 0x72a8_14b3_u64;
    for index in 0..4096 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut bytes = seeds[index % seeds.len()].to_vec();
        let at = state as usize % bytes.len();
        match index % 3 {
            0 => {
                bytes.remove(at);
            }
            1 => bytes.insert(at, ((state >> 16) & 127) as u8),
            _ => bytes[at] = ((state >> 24) & 127) as u8,
        }
        let input = std::str::from_utf8(&bytes).unwrap();
        let result = std::panic::catch_unwind(|| {
            if let Ok(document) = html::parse(input)
                && let Ok(first) = render::render(&document, 32, 32)
            {
                let second =
                    render::render(&document, 32, 32).expect("same input must still render");
                assert_eq!(first.ppm, second.ppm);
                assert_eq!(first.layout_json, second.layout_json);
            }
        });
        assert!(result.is_ok(), "mutation {index} panicked on {input:?}");
    }
}
