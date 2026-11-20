# M1 scope and fixture contract

Contract **M1-v1**, frozen 16 November 2026. Machine-readable source:
[planning/m1-cohort.json](../planning/m1-cohort.json). This freezes the required
behavior and test inputs; it does not assert implementation completion or test passes.

M1 remains the deterministic offline renderer described in [ROADMAP.md](ROADMAP.md).
The first executable bootstrap is a deliberately smaller development slice. Its
results must always use the bootstrap denominator, separately from the full M1
cohort, upstream WPT, and eventual browser compatibility results.

## Frozen feature boundary

| Area | Full M1 requirement | Frozen evidence contracts |
| --- | --- | --- |
| HTML input | UTF-8 document and `div` fragment contexts; standard document/head/body creation, doctype, text and attributes; character references, duplicate attributes, raw-text tokenization and malformed paragraph recovery | H001–H006 specify exact tree/value expectations; named upstream parser fixtures are pinned in the manifest |
| DOM | Stable node identity and parent/child ownership; element/attribute/text access; append/remove/reparent invariants and bounded teardown | Inspect fixtures prove the static tree; ownership/mutation tests must also pass before B013 closes |
| CSS syntax | Rule/declaration tokenization and recovery, comments, quoted values, invalid declarations and unknown properties | C003 rejects invalid values while retaining subsequent valid declarations; relevant syntax sections are linked per case |
| Selectors and cascade | Type, class, ID and compound selectors; descendant, child, adjacent and general sibling combinators; UA/author/user origins, importance, specificity, source order, inline style, inheritance and initial values | C001–C003 plus B003 and pinned selector/cascade upstream cases |
| Layout | Content/border box dimensions, normal block and inline flow, positive/negative/collapsing margins, padding, solid borders, whitespace collapse and line wrapping | B001/B002 and L001/L002 give geometry, text/line and pixel assertions; B016 also needs nested/empty/oversized and margin-collapse unit coverage |
| Text | Pinned Noto Sans Latin, Noto Sans Arabic and Noto Sans Devanagari; OpenType shaping, correct direction, clusters and glyph positioning; no missing-glyph substitution or system fallback in fixtures | T001–T003 compare complete independent reference glyph runs; T004/T005 exercise document text integration |
| Paint/resources | Deterministic CPU display list and pixels; colors, backgrounds, solid borders, clipping, controlled PNG decode and alpha compositing | B001–B003 and P001–P003 specify exact pixel samples and invalid-image handling |
| Harness | Constrained offline resources, structured failures, exact environment identity, 100 repetitions, second equivalent worker, seven-day parser/layout fuzz campaign | R001–R004 define boundary inputs; milestone evidence must additionally satisfy ROADMAP.md and VALIDATION.md |

Identifiers above omit the common `M1-` prefix. These are feature cohorts, not a
claim that all of HTML or CSS is supported. Tables, flex/grid, floats, positioned
layout, advanced effects, scripting, arbitrary resource fetching, platform UI and
accessible interaction expand in their assigned later gates. Unsupported features
must not produce success-shaped output or be silently omitted by the bootstrap.

## Bootstrap contract

The first runner operates only on controlled fixtures in
`fixtures/m1/bootstrap`. B001 draws one red rectangle; B002 verifies nested content,
padding, border and margin coordinates; B003 verifies a concrete cascade result.
B004 requires explicit rejection of visible text before text rendering exists.
B005 requires rejection of an external stylesheet request without fetching it.

The manifest represents pixels as `[x, y, red, green, blue]`, zero-based from the
viewport's top-left corner. Rectangles use `[x, y, width, height]` in integer CSS
pixels. All bootstrap fixtures explicitly set the body's margin to zero. Their
PPM image format is a development output; PNG resource support remains a separate
full-M1 requirement. A renderer that paints empty rectangles while omitting text
does not pass a text case.

`execution: "not_run"` in the frozen contract is not a current-results database.
Each actual runner invocation writes a separate evidence report with the commit,
toolchain, environment and per-case outcomes. Do not edit the contract to erase
failures or count deferred cases as passes.

## Resource contract

| Resource | Maximum accepted value | Required boundary behavior |
| --- | --- | --- |
| HTML UTF-8 input | 1,048,576 bytes | Reject maximum + 1 before unbounded allocation; malformed/unsupported content has its own failure |
| Document nodes | 10,000 | Count document/doctype/element/text nodes, including generated nodes; never truncate silently |
| Element depth | 128 | Count elements from html through the current node inclusive; exclude the document node |
| Viewport width or height | 2,048 CSS pixels | Reject zero and maximum + 1 before allocating a surface |
| CSS source total | 262,144 bytes | Bound style parsing independently of HTML length |
| Decoded resource bytes | 16,777,216 bytes per resource | Validate before allocating decoded image/font work buffers |
| Process resident memory | 536,870,912 bytes | Isolated runner terminates and reports a limit failure |
| Fixture wall time | 5 seconds | Runner terminates the child and records timeout, never pass |

R001–R004 define exact below/at/above values and reproducible generator recipes.
These are implementation obligations, not claims that every bound is already
enforced. RSS and wall-clock enforcement belong to the isolated runner; input,
node, depth, decoded-size and viewport bounds belong at engine entry/allocation
boundaries. The bootstrap reports its implemented checks separately. Its lack of
network calls does not constitute an OS sandbox or authorization to open arbitrary
untrusted local documents.

## Assets and independent oracles

[Asset provenance](../fixtures/m1/assets/README.md) and the manifest record source
revisions, sizes, SHA-256 digests and licenses. The PNG fixture is a hand-authored
2×2 RGBA matrix; its expected bytes and pixels were chosen independently of the
engine. A deliberately truncated PNG exercises decoder failure.

The three unmodified Noto fonts and their original SIL Open Font License 1.1 files
are bundled, pinned to Google Fonts commit
`cc39c4c2c304bc368c9b7c311ea2bcc6013299fe`. They retain that license. Font
instances use width 100 and weight 400. Unicode 17.0.0 is the selected Unicode-data
baseline; this does not claim full Unicode conformance.

Independent shaping references were generated with installed **HarfBuzz 14.3.1**,
OpenType shaper/functions, scale 1024, explicit script/language/direction and UTF-8
byte cluster offsets. Each expected JSON file retains the exact command arguments
and full glyph IDs, offsets and advances. At 16px font size, scale 1024 represents
64 positioning units per CSS pixel. These are reference-shaper outputs, not BroWSER
test results. Changes to a font, shaping configuration or oracle require a reviewed
contract revision. Screenshot references for text still need independent raster
review when font painting is integrated; engine-generated images cannot approve
their own correctness.

## Upstream identities and change control

The manifest pins WHATWG HTML source commit
`61a2f78025e7ac78849c172d276f132d9f2fe8a3` and WPT commit
`8746bafe086e036b5e5be028ac200e4a595ef3d0`. Eleven exact WPT file IDs were verified
at that revision and listed as semantic reference material. They are not bundled,
not executed by the bootstrap and not counted as upstream harness passes. Some
need JavaScript or XHTML test infrastructure beyond M1; authored derived semantics
remain explicitly distinct from those upstream tests.

The local cases are original MIT fixtures and contain no copied upstream test
source. The complete required feature boundary includes the stated unit/property
coverage as well as the 28 enumerated integration cases: passing only those small
integration examples cannot close B012–B019. No fixture or future outcome can be
removed from the denominator after measurement without a visible scope revision.

Primary references checked 16 November 2026:

- [HTML tokenization and tree construction](https://html.spec.whatwg.org/multipage/parsing.html).
- [CSS Syntax Level 3, 24 December 2021 snapshot](https://www.w3.org/TR/2021/CRD-css-syntax-3-20211224/).
- [Selectors Level 3, 6 November 2018 Recommendation](https://www.w3.org/TR/2018/REC-selectors-3-20181106/).
- [Cascade Level 5, 13 January 2022 snapshot](https://www.w3.org/TR/2022/CR-css-cascade-5-20220113/).
- [CSS2 box model](https://www.w3.org/TR/2011/REC-CSS2-20110607/box.html) and [visual formatting](https://www.w3.org/TR/2011/REC-CSS2-20110607/visuren.html).
- [HarfBuzz documentation](https://harfbuzz.github.io/) and [Unicode 17.0.0](https://www.unicode.org/versions/Unicode17.0.0/).
