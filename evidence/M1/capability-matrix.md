# M1 implementation status

Updated 21 November 2026. **M1 is in progress.** B010's scope/fixture contract is
complete. B011's clean Linux and macOS builds and bootstrap checks passed in
[CI for e1c0523](https://github.com/pookie421/BroWSER/actions/runs/44714692984), so
bootstrap setup is complete. The frozen M1-v2 cases now run through the offline
fixture adapter. [Current roadmap status](../../planning/roadmap.json)

| Capability | Working fixture behavior | Remaining obligation |
| --- | --- | --- |
| Input and DOM | Bounded UTF-8, doctype nodes, document/div-fragment tree construction, stable document-local handles, checked attachment and detachment, text and attributes, bounded arena teardown | Broader HTML states, references and live DOM operations as the engine grows |
| CSS | Author/user/UA cascade, token-aware grammar and recovery, computed-value inspection, inheritance and initial values | Broader selector/value and display coverage |
| Geometry | Integer block boxes, adjoining vertical margin collapse, clipping, and whitespace collapse and wrapping for standalone pinned-font text | Mixed inline/element flows and broader block/inline conformance |
| Paint | CPU RGB PPM, clipped solid fills, canvas backgrounds, PNG alpha compositing and grayscale glyph paint | Independent text raster review and later isolated codec execution |
| Text/resources | Real OpenType shaping and coverage from the three pinned Noto faces, declared 8-bit static PNGs with bounded decoded size | Mixed-direction itemization, additional scripts, and broader resource formats |
| CLI | Inspect, style, shape, decode-png and render commands, constrained fixture roots and atomic evidence bundles. Fixture runs use a fresh scratch environment and can record and enforce sampled per-child RSS. | Linux worker isolation and live Linux RSS backend verification |
| Tests | Build-bound bootstrap and full-M1 replay, typed oracles, same-process text/image replay, verified replay comparison, and a bounded concurrent fuzz driver | Second equivalent worker and sustained coverage-guided fuzzing |
| Scripting/security | Inert script fixture data; initial VM source/build-prerequisite research; a separate [offline M2 broker experiment](../../docs/M2_SCOPE.md#offline-broker-experiment) | VM embedding, network service, OS sandbox and general browsing support remain future work |

The M1-v2 fixture roster stays frozen. The full-M1 replay checks exact shaped
glyphs and retained screenshots for T004/T005. These two cases await independent
raster review. WPT execution remains at zero.
[Frozen scope](../../docs/M1_SCOPE.md) · [Replay policy](../../docs/VALIDATION.md#evidence-identity)

| Local verification on macOS arm64 with Rust 1.100.0 | Observed result |
| --- | --- |
| Rust and Python suites | 124 Rust tests and 75 Python tests passed. Formatting, Clippy and plan validation passed. |
| Bootstrap | 5 of 5 passed, with 100 repetitions per render case. |
| Full M1 | 21 passed and 2 await raster review. Two RSS-monitored replays on the same worker matched all 23 case outcomes and hashes across 100 repetitions per case and 8,200 fixture invocations. No assertion, resource, timeout, crash or harness failures were recorded. |
| Independent margin comparison | All 769 labeled border rectangles from 45 generated in-flow block trees matched WebKit. |
| Parser and renderer fuzz smoke | 5,000 AddressSanitizer-instrumented inputs per target completed under the campaign driver. A short timed run and supervised interruption also exercised telemetry and cleanup. |

The monitored fixture runs had a maximum measured peak of 8,814,592 bytes.
A separate synthetic over-limit child was terminated when a sample crossed
the 512 MiB cap. The observed peak was 542,785,536 bytes. Independent
HarfBuzz and FreeType references matched the frozen glyph geometry and both
ink bounds. Arabic and Devanagari raster acceptance, the seven-day campaign,
Linux isolation and a second equivalent worker remain open.
[Fuzz and replay evidence requirements](../../docs/VALIDATION.md#determinism-bounds-and-adversarial-runs)
