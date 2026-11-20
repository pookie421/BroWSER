# M1 implementation status

Updated 20 November 2026. **M1 is in progress.** B010's scope/fixture contract is
complete. B011's clean Linux and macOS builds and bootstrap checks passed in
[CI for e1c0523](https://github.com/pookie421/BroWSER/actions/runs/44714692984), so
bootstrap setup is complete. B012/B014 are in progress; B030 feasibility is in
progress alongside M1. Wider task gates remain open until their full contracts pass.

| Capability | Working bootstrap behavior | Remaining M1 obligation |
| --- | --- | --- |
| Input and DOM | Bounded UTF-8 with normalized offsets, doctype nodes, strict parser, opt-in document and div-fragment inspection with bounded paragraph recovery | Broader HTML states and references, stable DOM mutation and teardown coverage |
| CSS | Token-aware rule and declaration boundaries, tree-ordered screen stylesheets, bounded selectors, author cascade, foreground inheritance for paint and precise unsupported-feature diagnostics | Complete frozen syntax recovery, user origin and computed-style output |
| Geometry | Integer block content boxes, edges, selected widths/heights, hidden elements and overflow clipping | Inline formatting, whitespace/wrapping, margin collapse and full geometry cohort |
| Paint | One CPU RGB PPM buffer, bounded clipped fills, canvas propagation and solid border defaults | Display-list contract breadth, text rasterization, PNG decode and compositing |
| Text/resources | Real pinned Noto assets, original notices and independent HarfBuzz references exist | No visible text or PNG resources are rendered yet; unsupported requests fail explicitly |
| CLI | Inspect/render commands, constrained fixture roots, bounded reads, staged output and rollback diagnostics | Full M1 fixture adapter and isolated RSS enforcement |
| Tests | Build-bound bootstrap replay with fresh evidence bundles, exact rejection/control checks, typed DOM/layout validation, and a Unicode mutation smoke test | Official full-M1 execution, broader unit/property obligations, second equivalent worker and seven-day coverage-guided fuzzing |
| Scripting/security | Inert script fixture data; initial VM source/build-prerequisite research; a separate [offline M2 broker experiment](../../docs/M2_SCOPE.md#offline-broker-experiment) | VM embedding, network service, OS sandbox and general browsing support remain future work |

The M1-v2 audit follow-up passed 88 Rust and 49 Python tests locally on macOS
arm64 with Rust 1.100.0. All five bootstrap cases passed, including 100
identical repetitions for each of the three render cases and both exact
rejection/control pairs. The pixel hashes match M1-v1. The layout hashes changed
because inspection now counts the doctype as a document node.

Direct checks matched H001–H006, L002, P001 and R001–R004. The official
full-M1 roster still has 23 cases awaiting execution. WPT execution remains
at zero. [Audit results](../../docs/CODE_AUDIT.md#remediation) · [Bootstrap evidence](BOOTSTRAP.md)
