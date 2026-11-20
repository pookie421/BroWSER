# First executable M1 slice

Observed 20 November 2026 on macOS arm64 with pinned Rust 1.100.0. This is the first
complete controlled-fixture parser/style/layout/paint path. M1 remains in progress.

## Implemented and exercised

The CLI reads a bounded UTF-8 fixture inside its configured fixture root, creates
an owned document tree, resolves the supported CSS declarations and selectors,
computes block rectangles, paints a bounded RGB surface, and writes PPM/DOM/layout
artifacts. It never advertises text rendering or resource loading that is absent.
Malformed/unsupported fixture requirements return errors.

Regression checks caught and fixed external stylesheets silently producing output,
transparent border painting, default border styles, auto-height clipping, text-node
depth accounting and aggregate CSS-source limits. Independent geometry and pixel
expectations were preserved; no failing case was removed or counted as passing.

| Check | Observed result |
| --- | --- |
| Python contract/evidence tests | 34 passed |
| Rust parser/renderer unit tests | 38 passed |
| Executable CLI integration tests | 6 passed |
| Deterministic mutation smoke test | 1 passed, covering 4,096 generated mutations |
| Rust formatting and Clippy | Passed; warnings denied |
| Frozen bootstrap integration cases | 5 passed: 3 renders and 2 required rejections |
| Render determinism | Each render case passed 100 independent CLI invocations |
| Full-M1 integration cases | 23 not run |
| WPT | 0 executed |
| Linux/macOS CI for this source | Pending publication verification |

Commands exercised:

```sh
python3 -m unittest discover -s tests -v
cargo fmt --all -- --check
cargo test --locked --offline --all-targets
cargo clippy --locked --offline --all-targets -- -D warnings
cargo build --locked --offline
python3 tools/run_m1_bootstrap.py --repeat 100
```

## Retained identity and raw results

- [Local result manifest](2026-11-20-macos/bootstrap-report.json): compiler identity,
  source/cohort/runner/binary/input hashes, UTC times, environment and per-case results.
- [Command diagnostics](2026-11-20-macos/bootstrap-report.commands.json): every invoked
  fixture command, exit code, stdout and stderr; temporary output directory names
  are normalized to a documented placeholder.
- [Frozen cohort](../../planning/m1-cohort.json): 28 case contracts, 34 hashed assets,
  source revisions and independent expected geometry/glyph/pixel values.

The local manifest records base commit `691b825` with `worktree_dirty: true` because
the implementation had not yet been committed. Its source hashes identify the
actual tested files. It does not claim the old planning commit contained this code.
The published CI workflow replays the exact committed source on Linux and macOS
and retains development reports/diagnostics for 90 days as Actions artifacts.

## Gates still open

This bootstrap is unsandboxed and accepts controlled synthetic fixtures only.
Fixture path containment and absence of network calls do not demonstrate an OS
sandbox. RSS containment, broader conformance, text shaping/painting, PNG resources,
the second equivalent worker, and seven-day sustained fuzzing remain unproven.
The short mutation test is not a substitute for the required fuzz campaign.

B030 has started source and environment investigation in
[VM_FEASIBILITY.md](../../docs/VM_FEASIBILITY.md). No VM has been compiled or selected,
and no script execution, network browser or desktop app is claimed.
