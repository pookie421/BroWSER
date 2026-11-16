# Standards and research sources

Primary references consulted on 16 November 2026. These links are a starting map,
not immutable implementation targets. Each implementation work item must record
the exact specification section and upstream test revision it uses. Living standards
and upstream tools can change; recheck them when freezing a milestone cohort.

| Reference | Application |
| --- | --- |
| [HTML Standard](https://html.spec.whatwg.org/multipage/) | Parsing, document lifecycle, navigation and event-loop semantics |
| [DOM Standard](https://dom.spec.whatwg.org/) | Trees, mutation, events and object behavior |
| [CSS specifications](https://www.w3.org/Style/CSS/specs.en.html) | Select the exact syntax, cascade, layout and rendering modules for each cohort |
| [URL Standard](https://url.spec.whatwg.org/) | URL parsing, serialization and origin-related integration |
| [Fetch Standard](https://fetch.spec.whatwg.org/) | Request/response policy and cross-origin behavior |
| [Web Platform Tests](https://web-platform-tests.org/) | Shared interoperability tests and runner documentation |
| [Official Test262 suite](https://github.com/tc39/test262) | ECMAScript conformance and execution interpretation |
| [SpiderMonkey](https://spidermonkey.dev/) | Candidate established VM and embedding entry points |
| [V8 embedding](https://v8.dev/docs/embed) | Alternative VM integration research |
| [HarfBuzz manual](https://harfbuzz.github.io/) | Text shaping integration research |

The architecture, security, roadmap and validation documents link additional primary
sources at the claims they inform. BroWSER's thresholds, dates, staffing scenarios,
process boundaries and policy choices are project proposals. Upstream sources do
not endorse the project or certify its readiness.

## Evidence rules

Do not substitute a blog summary, generated answer, screenshot of a demo, or planned
test for implementation evidence. Keep upstream test origins and licenses when deriving
fixtures. Separate normative behavior, a BroWSER design decision, an estimate, and an
observed result in reviews. No competitive score or industry resource estimate is
asserted in this baseline.
