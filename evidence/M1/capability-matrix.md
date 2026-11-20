# M1 implementation status

Updated 20 November 2026. **M1 is in progress.** B010's scope/fixture contract is
complete. B011 has a working macOS bootstrap; its clean Linux CI build is pending
publication verification. Wider task gates remain open until their full contracts pass.

| Capability | Working bootstrap behavior | Remaining M1 obligation |
| --- | --- | --- |
| Input and DOM | Bounded UTF-8, explicit balanced tags, attributes/references, parent/child ownership, stable JSON | HTML insertion modes, document/fragment recovery, broader references and tested DOM mutation |
| CSS | Bounded embedded/inline source; compound type/ID/class, child/descendant selectors, author cascade, importance and inline precedence | Complete frozen syntax recovery, sibling combinators, UA/user origins, inheritance and computed-value coverage |
| Geometry | Integer block content boxes, edges, selected widths/heights, hidden content and overflow clipping | Inline formatting, whitespace/wrapping, margin collapse and full geometry cohort |
| Paint | CPU RGB surface, solid backgrounds/borders, clipping, P6 PPM output | Display-list contract breadth, text rasterization, PNG decode and compositing |
| Text/resources | Real pinned Noto assets, original notices and independent HarfBuzz references exist | No visible text or PNG resources are rendered yet; unsupported requests fail explicitly |
| CLI | Real inspect/render commands, constrained fixture roots, bounded read, staged non-overwriting files, JSON diagnostics | Full M1 fixture adapter and isolated resource/RSS enforcement |
| Tests | 38 library tests, six executable tests, one deterministic mutation smoke test; five frozen bootstrap cases | 23 full-M1 integration cases not run, broader unit/property obligations, second equivalent worker and seven-day coverage-guided fuzzing |
| Scripting/security | Inert script fixture data; initial VM source/build-prerequisite research | No VM embedding, network service, OS sandbox or general browsing support |

All three render fixtures passed their independent geometry/pixel assertions and
100 repeated output-hash checks. Both negative fixtures returned explicit errors
without artifacts. This is **5/5 bootstrap cases**, not a full-M1 or WPT score.
WPT executed: **0**. See [the evidence index](BOOTSTRAP.md) and the
[frozen scope](../../docs/M1_SCOPE.md).
