# M1 scope and fixture contract

Contract **M1-v2** revises the 16 November 2026 freeze on 20 November 2026.
The machine-readable manifest freezes the required behavior and test inputs.
[Current cohort](../planning/m1-cohort.json) · [M1-v1 snapshot](https://github.com/pookie421/BroWSER/blob/c97fa6a747638177fc2a618ada27eb62eb27d6ef/planning/m1-cohort.json)

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

The default runner uses the bootstrap profile on fixtures in
`fixtures/m1/bootstrap`. B001 draws one red rectangle; B002 verifies nested content,
padding, border and margin coordinates; B003 verifies a concrete cascade result.
B004 checks the bootstrap profile's explicit rejection of visible text.
B005 requires rejection of an external stylesheet request without fetching it.
Both require exit 1 with the fixture's exact diagnostic, no output files, and
successful rendering of a paired control that removes the unsupported feature.

The manifest represents pixels as `[x, y, red, green, blue]`, zero-based from the
viewport's top-left corner. Rectangles use `[x, y, width, height]` in integer CSS
pixels. All bootstrap fixtures explicitly set the body's margin to zero. Their
PPM image format is the development output. The full cohort checks PNG input
decoding independently and requires text cases to produce visible glyphs.

`execution: "not_run"` in the frozen contract is not a current-results database.
Each actual runner invocation writes a separate evidence report with the commit,
toolchain, environment and per-case outcomes. Do not edit the contract to erase
failures or count deferred cases as passes.

The CLI's default parser uses balanced explicit tags. The inspect-only document
mode supports implied `html`, `head` and `body` and bounded paragraph recovery.
The inspect-only div fragment mode returns the parsed children under the JSON root.
Both modes use the same input, token and tree limits as strict parsing.
They accept the subset of head metadata and ordinary block and phrasing elements
documented by the parser. Other tree-construction modes produce a diagnostic.
[Tree-construction implementation](../src/html/tree.rs)

## Current engine profiles

- `inspect --parse-mode document` and `inspect --fragment-context div` expose the
  bounded document and fragment modes. The library provides document-owned handles
  and validates ownership, cycles and depth before append, move or removal.
  Detaching a subtree preserves its handles and hides it from inspection and
  rendering. Detached nodes still count against the arena budget. Replacing text
  or attributes reclaims the bytes of the previous strings.
- `style --user-css CSS` lists computed values in tree order. The user sheet is
  optional. Author rules beat normal user declarations, and important user rules
  beat important author declarations. Supported properties resolve initial and
  inherited values, including `initial`, `inherit`, and `unset`.
- `shape --script Latn|Arab|Deva --direction ltr|rtl --language TAG --scale 1024`
  reads UTF-8 fixture text and reports glyph IDs, UTF-8 clusters and signed
  positions from the pinned font.
- `render --profile m1` uses document parsing and formats standalone block text in
  a single supported script and direction. It collapses ASCII whitespace, wraps
  using shaped advances, records line and glyph geometry, and paints grayscale
  coverage into the clipped RGB surface. It collapses adjoining vertical margins
  for in-flow blocks, including empty boxes and eligible parent-child edges.
  Borders, padding, and overflow formatting contexts separate those edges.
  Mixed inline elements, mixed-direction runs, and non-pinned fonts remain
  outside the supported slice. [CSS collapsing margins](https://www.w3.org/TR/CSS22/box.html#collapsing-margins)
- `render --resource NAME=PATH` maps an explicit label to a PNG within the fixture
  root. The image renderer supports static 8-bit RGB and RGBA with untagged
  or sRGB color. It composites alpha onto the existing surface, uses intrinsic
  dimensions by default, and resizes and clips the image to the fixture box.
  Standalone inline images and explicit block images are supported. The
  `decode-png` command verifies container and compressed-data checksums and
  reports decoded dimensions.

## Resource contract

| Resource | Maximum accepted value | Required boundary behavior |
| --- | --- | --- |
| HTML UTF-8 input | 1,048,576 bytes | Reject maximum + 1 before unbounded allocation; malformed/unsupported content has its own failure |
| Document nodes | 10,000 | Count document/doctype/element/text nodes, including generated nodes; never truncate silently |
| Element depth | 128 | Count elements from html through the current node inclusive; exclude the document node |
| Bootstrap attribute tokens | 128 per start tag | Count all tokens including duplicates. Retain the first value for each name. Reject the 129th token. |
| Viewport width or height | 2,048 CSS pixels | Reject zero and maximum + 1 before allocating a surface |
| CSS source total | 262,144 bytes | Bound style parsing independently of HTML length |
| Decoded resource bytes | 16,777,216 bytes per resource | Validate before allocating decoded image/font work buffers |
| Process resident memory | 536,870,912 bytes | Isolated runner terminates and reports a limit failure |
| Fixture wall time | 5 seconds | Runner terminates the child and records timeout, never pass |

R001–R004 define exact below/at/above values and reproducible generator recipes.
Input, node, depth, decoded-size and viewport bounds belong at engine allocation
boundaries. The full-M1 replay imposes the five-second timeout and samples
per-child RSS, with a kernel peak check when it reaps each invocation. Dedicated
worker isolation remains required for acceptance and the developer tool remains
limited to controlled input. [Fixture and worker evidence](VALIDATION.md#evidence-identity)

The input limit counts the original bytes, including a leading UTF-8 signature.
The parser strips one leading signature, preserves U+FEFF elsewhere, and
normalizes CRLF and lone CR to LF. Diagnostics report zero-based UTF-8 byte
positions in that normalized stream, including for invalid character references.

The development renderer charges up to 5,120,000 selector checks across all
applicable rules. Each compound costs at least one check per element, with
additional charges for tag, ID and class probes when a compound has several.
The separate sibling-selector check retains the 5,120,000 element–compound
limit for `+` and `~`. Rules with no usable declarations incur no match work.
Cascade processing has a separate 5,120,000 matched-declaration limit.
Paint preflights up to 33,554,432 clipped pixel writes across canvas,
backgrounds, borders, images and glyphs before allocating one RGB PPM buffer.
The fixture resource map retains up to 64 PNGs and 32 MiB of normalized image
data. The decoder checks `width × height × 4` against the per-resource limit
before frame allocation. Inline layout bounds per-document text bytes, shaped
glyph count, repeated shaping work and retained glyph coverage.

Embedded stylesheets are parsed independently in document order. The bootstrap
uses a screen context with bare `all`, `screen`, `print` and `speech` media
types and comma-separated lists. It ignores non-CSS and nonmatching sheets
and diagnoses media query syntax outside the supported subset. Unknown
declarations and invalid values in the supported subset follow CSS recovery.
Recognized unsupported layout or paint behavior returns a diagnostic.

An ordinary `hidden` attribute removes its element from paint and flow unless
an explicit author `display` declaration overrides it. A root background
paints the canvas. With a transparent root, the document body's background
paints the canvas. Solid borders with omitted width use 3px in this renderer.
The initial border color follows the element's foreground.

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
review. The renderer records screenshots and exact glyph geometry so the
reviewer can compare them with independent images and pin the accepted evidence.

### Runtime dependencies

Cargo pins HarfRust 0.13.3 (MIT) for OpenType shaping, Skrifa 0.46.2
(MIT or Apache-2.0) for face metrics and outlines, ab_glyph_rasterizer 0.1.10
(Apache-2.0) for grayscale coverage, and png 0.18.1 (MIT or Apache-2.0)
for bounded image decoding. First-party engine code uses their Rust interfaces.
The renderer loads embedded fixture fonts and never resolves system fonts.
Cargo checksums and the complete transitive versions are recorded in the lockfile.
[HarfRust](https://github.com/harfbuzz/harfrust/releases/tag/0.13.3) · [Skrifa](https://docs.rs/crate/skrifa/0.46.2) · [ab_glyph_rasterizer](https://docs.rs/crate/ab_glyph_rasterizer/0.1.10) · [image-rs PNG](https://github.com/image-rs/image-png) · [Cargo.lock](../Cargo.lock)

The text and rendering maintainers own dependency updates. Preserve upstream
notices, review lockfile and license changes, and repeat the independent glyph,
pixel and malformed-input checks before accepting a revision. Fixture decoding
runs inside the development process. Codec isolation and broader decode fuzzing
remain release gates.

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

### M1-C001 specificity discrepancy

M1-v1 specified a green background for `#b`. Both `main > div.item` and
`div + div` match it, with respective specificities `(0, 1, 2)` and `(0, 0, 2)`.
They have equal origin and importance, so the first rule wins and M1-v2
expects red. The revised fixture adds an unclassed adjacent sibling, `#c`,
whose green background independently exercises the adjacent selector.
[Revised input](../fixtures/m1/full/selectors-v2.html) · [Selector specificity](https://www.w3.org/TR/2018/REC-selectors-3-20181106/#specificity) · [Cascade sorting order](https://www.w3.org/TR/2022/CR-css-cascade-5-20220113/#cascade-sort)

M1-v2 keeps all 28 case IDs and records the superseded manifest digest.
M1-v1 results remain historical. The revised C001, B004 and B005 assertions
require new evidence under M1-v2. No official C001 run was recorded against M1-v1.

Primary references checked 16 November 2026:

- [HTML tokenization and tree construction](https://html.spec.whatwg.org/multipage/parsing.html).
- [CSS Syntax Level 3, 24 December 2021 snapshot](https://www.w3.org/TR/2021/CRD-css-syntax-3-20211224/).
- [Selectors Level 3, 6 November 2018 Recommendation](https://www.w3.org/TR/2018/REC-selectors-3-20181106/).
- [Cascade Level 5, 13 January 2022 snapshot](https://www.w3.org/TR/2022/CR-css-cascade-5-20220113/).
- [CSS2 box model](https://www.w3.org/TR/2011/REC-CSS2-20110607/box.html) and [visual formatting](https://www.w3.org/TR/2011/REC-CSS2-20110607/visuren.html).
- [HarfBuzz documentation](https://harfbuzz.github.io/) and [Unicode 17.0.0](https://www.unicode.org/versions/Unicode17.0.0/).
