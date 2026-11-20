# BroWSER Code Audit and Fix Plan

Audited on 20 November 2026. The review covers the local checkout based on `c97fa6a747638177fc2a618ada27eb62eb27d6ef`, including the pending title, sibling-selector, and offline broker changes.

## Remediation

This follow-up addresses the 26 findings below. The original repros and the
6-of-23 direct baseline remain in the report for comparison. M1-v2 retains the
28 case IDs and the M1-v1 history. [Contract revision](M1_SCOPE.md#m1-c001-specificity-discrepancy)

| Findings | Delivered behavior and regression coverage |
| --- | --- |
| CSS-01–CSS-03 | Recognized unsupported behavior fails before either CLI output is published. The token scanner preserves comments' boundaries and handles quoted delimiters. Invalid lengths, display keywords and hex colors exercise recovery. |
| CSS-04–CSS-06 | Screen and type applicability, complete border shorthand resets, and independent style sheets collected in document order. Reordering nodes preserves their IDs and matches freshly parsed cascade results. |
| RENDER-01–RENDER-02 | Hidden nodes leave paint and flow, root/body backgrounds propagate to the canvas, and matched declarations and clipped fills have work bounds. Boundary tests exercise accepted and rejected workloads. |
| HTML-01–HTML-04 | One leading UTF-8 signature is stripped, diagnostics use normalized byte positions, and the strict doctype and attribute rules are tested and documented. Doctype nodes are present in inspection and count toward the limit. |
| EVID-01–EVID-04, EVID-07 | Exact rejection and passing control checks, pinned manifest and input identities, fresh Cargo executable provenance, unique evidence bundles and alternate-target coverage. |
| EVID-05–EVID-06, EVID-08 | Attempts retain diagnostics, timing and available output hashes with typed outcomes. DOM/layout validation rejects malformed structures and verifies pixel-only cases. Repository link checks exclude local handoff files. |
| CONTRACT-01 | M1-v2 independently asserts the red specificity winner and the green adjacent-only match. The engine exercises both winners in a pixel control. |
| CLI-01–CLI-02, TEST-01 | Injected late publication and cleanup failures exercise rollback and affected-path diagnostics. CLI tests check exact geometry, pixels and both viewport dimensions. The mutation smoke test reaches deterministic Unicode parse and render comparisons for every seed and edit type. |
| IPC-01–IPC-02 | The synthetic broker replay bounds I/O and exit and reaps both subprocesses after a stall. Independent byte fixtures verify complete success and denial frames. |

The revised inspection modes matched the six HTML case oracles in direct CLI
checks. L002, P001 and all four R cases also matched, for **12 of 23 direct
matches**. The selector check used a text-free C001 control and covers only
its background assertions. The official runner passed **5 of 5 bootstrap
cases** on macOS arm64 with Rust 1.100.0, with 100 identical repetitions per
render case. The local suites passed 88 Rust tests and 49 Python tests.
Isolated CLI checks below, at and above the match and paint limits ran within
0.39 seconds and 15 MB of peak resident memory. Above-limit cases returned the
expected diagnostic with neither output. Full-M1 execution, isolated RSS
enforcement, second equivalent worker and sustained fuzzing remain open.
[Current capability matrix](../evidence/M1/capability-matrix.md)

## Original audit

The current offline bootstrap passed all five frozen development fixtures. Small fixtures outside that set exposed incorrect rendering, and the runner can accept an unrelated failure as evidence that it rejected an external stylesheet. Fix that assertion, stop accepting incomplete renders for recognized unsupported CSS, and bind fixture reports to the builds and cohort they actually exercised before using the results to close M1 gates.

## How to prioritize the findings

| Priority | Meaning |
| --- | --- |
| P1 | Wrong successful output or unreliable evidence for a required gate. Fix before relying on the affected behavior or result. |
| P2 | Confirmed correctness, reliability, or coverage problem with a narrower impact. Schedule with the owning subsystem. |
| P3 | Diagnostics, maintenance, or test coverage improvement. Include with nearby changes. |

The audit has **26 findings** (6 P1, 14 P2, and 6 P3). Each gives evidence, a proposed change, and a condition for closing it. **Reproduced** means an ordinary local fixture or harness check demonstrated the result. **Code review** means the impact follows from the source and still needs a targeted regression.

[Engine](#engine-findings) · [Evidence and contracts](#evidence-and-contract-findings) · [CLI and regression coverage](#cli-and-regression-coverage-findings) · [Broker experiment](#offline-broker-experiment-findings) · [Remaining M1 work](#remaining-m1-work) · [Fix order](#fix-order)

## Baseline checks

The local checks used macOS arm64, Rust 1.100.0, and Python 3.15.2. The audit covered the Rust parser, DOM, renderer, CLI and broker example, plus the Python validators, tests, frozen cohort, evidence and CI configuration.

| Check | Observed result |
| --- | --- |
| `make check` | 34 Python tests passed. Roadmap schema, dependency graph, evidence paths and local Markdown file targets passed validation. |
| `make engine-check` | Formatting, 59 Rust tests, all-target Clippy with warnings denied, and the locked offline build passed. The Rust count includes 48 library, 6 CLI, 1 mutation smoke and 4 broker tests. |
| Bootstrap replay within `make engine-check` | 5/5 passed. The three render cases each had 100 stable image and layout hashes. Both negative cases returned the intended diagnostics in this run. |
| `cargo run --locked --offline --example m2_broker` | The two-child replay delivered the synthetic resource to its owner and denied the other child and the stale request. |
| Full M1 and WPT | 23 full-M1 cases reported as not run. Zero WPT cases executed. |

The standard Make target overwrote the default generated bootstrap report and command log during this audit. The committed historical report and the separately archived title-and-sibling run remain available. The fresh report's command-log hash was checked against the sidecar. See [EVID-04](#evid-04-reruns-replace-existing-evidence) for the retention defect and [the committed bootstrap record](../evidence/M1/BOOTSTRAP.md) for the prior CI and replay evidence.

## Engine findings

### CSS-01 Recognized unsupported CSS can produce a successful wrong image

**P1 · Reproduced · B014**

An empty red 2×2 box still renders at its original size and location when the fixture specifies `opacity:0`, `position:relative; left:2px`, or `min-width:3px`. Each probe exited successfully. The declaration fallback treats recognized layout and paint features like unknown property names. This violates the documented requirement to report unsupported content that would otherwise be silently omitted. [Declaration dispatch](../src/render.rs#L530-L649) · [Unsupported feature policy](M1_SCOPE.md#L25-L29)

**Fix and acceptance.** Define a single classification for supported, invalid, unknown, and recognized unsupported declarations. Diagnose recognized unsupported behavior that affects the fixture and leave both requested output paths absent. Keep ordinary unknown-property recovery. Verify that the unchanged red box succeeds as a control and that the existing five bootstrap results still pass.

### CSS-02 Comment and delimiter handling changes valid CSS

**P2 · Reproduced · B014**

`div.card/**/.active` is turned into a descendant selector by inserting whitespace for the comment. The matching two-class box stays white. In an embedded sheet, a quoted `}` in an ignored property's value is taken as the end of the rule and prevents valid declarations from rendering. Inline declarations also split on raw semicolons. [Rule splitting](../src/render.rs#L303-L329) · [Comment stripping](../src/render.rs#L355-L369) · [Declaration splitting](../src/render.rs#L487-L498) · [CSS Syntax comment consumption](https://www.w3.org/TR/2021/CRD-css-syntax-3-20211224/#consume-comments)

**Fix and acceptance.** Tokenize comments, strings and component values before splitting selector lists, rules and declarations. Preserve real whitespace and token boundaries. Test embedded and inline examples with quoted punctuation, the comment-separated compound, and a genuine descendant selector as the control. Keep the source and token limits in force.

### CSS-03 Invalid values sometimes abort an entire render

**P2 · Reproduced · B014**

`width:noise2` and `display:nonsense` each abort a fixture that has usable neighboring declarations. Length handling uses the presence of a digit to decide whether to abort. In the other direction, a later invalid `background-color:#+f00ff` overrides valid red and paints `#0f00ff` because radix parsing accepts the leading plus. [Value dispatch](../src/render.rs#L597-L647) · [Length parsing](../src/render.rs#L652-L674) · [Color parsing](../src/render.rs#L677-L703) · [CSS hexadecimal color](https://www.w3.org/TR/css-color-4/#hex-notation)

**Fix and acceptance.** Use CSS value grammar to distinguish invalid syntax, valid unsupported features, and numeric limits. Validate hexadecimal digits before conversion. A later invalid declaration must leave the earlier valid winning value in place and must allow later valid declarations to apply. Keep an explicit rejection for a valid unsupported percentage length and keep the maximum-length check. Use the same classification as CSS-01.

### CSS-04 Inapplicable style elements can overwrite the cascade

**P2 · Reproduced · B014**

A later red rule in `<style media="print">` or `<style type="text/plain">` overwrites a blue baseline in the normal render. The collector appends every style element's text without looking at its applicability. [Stylesheet collection](../src/render.rs#L269-L303) · [HTML style element](https://html.spec.whatwg.org/multipage/semantics.html#the-style-element)

**Fix and acceptance.** Ignore non-CSS style elements. Define the bootstrap's media context and either evaluate the declared supported media syntax or diagnose unsupported query forms. In a screen fixture, a print-only rule must leave the blue box unchanged. A matching screen rule and an applicable `text/css` rule must still turn it red.

### CSS-05 Border defaults and shorthand resets are incomplete

**P2 · Reproduced · B014 and B016**

Setting `border-style:solid` without a width leaves the computed edges at zero. `border:none` also leaves a previous border color in the cascade. A later explicit width and solid style can bring the old color back. Border style, width and color defaults should be resolved independently, and the shorthand resets omitted longhands before the next cascade winner is applied. [Style defaults](../src/render.rs#L59-L81) · [Shorthand expansion](../src/render.rs#L568-L585) · [Computed edges](../src/render.rs#L937-L963) · [CSS2 border shorthands](https://www.w3.org/TR/2011/REC-CSS2-20110607/box.html#border-shorthand-properties)

**Fix and acceptance.** Expand the accepted shorthand into complete longhand assignments, resolve the initial width and color deterministically, and zero used width when style is none. Assert the border-box dimensions and pixels for solid without width, a reset followed by style and width, and the existing transparent-border control.

### CSS-06 Reordering valid DOM nodes leaves stylesheet precedence stale

**P2 · Reproduced with the Rust library · B013 and B014**

Swapping two style elements in their parent's child list leaves equal-specificity precedence in the original order. Parsing a fresh document in the same tree order produces the opposite, correct winning color. Stylesheet collection iterates the node arena while layout and sibling matching already use child order. [Tree validation and order](../src/render.rs#L215-L266) · [Rule collection](../src/render.rs#L269-L303) · [Cascade sorting order](https://www.w3.org/TR/2022/CR-css-cascade-5-20220113/#cascade-sort)

**Fix and acceptance.** Collect applicable sheets in validated tree order and parse each sheet as an independent input. After reorder or reparent, verify that existing node identities are preserved and the winning declaration and pixels equal a freshly parsed equivalent tree. Add a sheet-boundary case so syntax from a later sheet cannot finish an earlier one.

### RENDER-01 HTML visibility and canvas background semantics are skipped

**P2 · Reproduced · B014, B016 and B017**

An ordinary `<div hidden>` with a red box style renders red. In a document with a small red body and a transparent root background, the rest of the canvas stays white. The bootstrap derives default visibility from the tag and paints all backgrounds into the item's border box. With the current strict fixture policy, these cases need either the relevant behavior or an explicit unsupported-feature diagnostic. [Bootstrap style defaults](../src/render.rs#L59-L81) · [Layout visibility](../src/render.rs#L977-L1017) · [Background painting](../src/render.rs#L1106-L1111) · [HTML hidden elements](https://html.spec.whatwg.org/multipage/rendering.html#hidden-elements) · [CSS2 canvas backgrounds](https://www.w3.org/TR/2011/REC-CSS2-20110607/colors.html#background)

**Fix and acceptance.** Cover the normal hidden state with a visibility rule and decide which HTML background propagation cases belong in the bootstrap. Test a hidden element's lack of paint and flow, and use pixels inside and outside the body's box to distinguish canvas propagation from ordinary box fill. Specify the behavior of explicit CSS overrides in the fixture grammar.

### RENDER-02 Matching and painting have incomplete work accounting

**P2 · Code review · B014, B017 and B019**

The documented 5,120,000-check preflight covers sibling-containing selectors. Ordinary selectors still go through the per-element rule loop, including rules with no effective declarations. Paint iterates clipped pixels for every overlapping fill and the PPM path builds a separate RGB buffer before copying it into the result. The current audit did not measure a wall-time or memory failure. [Sibling budget](../src/render.rs#L734-L750) · [Ordinary selector matching](../src/render.rs#L828-L879) · [Surface allocation](../src/render.rs#L193-L211) · [Fill loop](../src/render.rs#L1096-L1141)

**Fix and acceptance.** Measure work for the entire match and paint pipeline under B019's isolated budget. Skip rules with no effective declarations and avoid redundant allocation or copying where measurements justify it. Document the accounting and add benign below, at, and above-boundary controls. Preserve the existing sibling check and deterministic geometry and pixels.

### HTML-01 A leading UTF-8 signature becomes visible text

**P2 · Reproduced · B012**

A UTF-8 BOM at the start of an otherwise renderable fixture appears as a root text node in inspect output. Render then fails with the visible-text diagnostic. The same document without the signature renders successfully. [Input preprocessing](../src/html.rs#L122-L134) · [Text rendering policy](../src/render.rs#L977-L982) · [HTML input byte stream](https://html.spec.whatwg.org/multipage/parsing.html#the-input-byte-stream)

**Fix and acceptance.** Strip one leading signature while still budgeting the full original input. Verify identical canonical DOM, layout, and PPM for equivalent files with and without the signature. Keep an interior U+FEFF as data and define its relationship to diagnostic offsets in HTML-02.

### HTML-02 Error offsets mix coordinate systems

**P2 · Reproduced · B012**

The parser promises normalized UTF-8 byte offsets. A NUL after CRLF is reported against the original bytes. For a bad reference after multibyte text, the reported byte is the start of the text, attribute value, or title content, several bytes before the offending reference. For example, `<p>é &copy;</p>` reports byte 3 while the reference begins at byte 6. [Offset contract and NUL check](../src/html.rs#L12-L17) · [Input preprocessing](../src/html.rs#L123-L133) · [Attribute value errors](../src/html.rs#L390-L391) · [Title and text errors](../src/html.rs#L451-L477)

**Fix and acceptance.** Have reference parsing report a relative error span, map it to a consistent documented byte coordinate system, and use that system when rejecting NUL. Assert exact error offsets for data text, attributes, and title with Unicode and CRLF. A single bounded preprocessing pass can also avoid the current pair of full-string replacements.

### HTML-03 The strict doctype grammar accepts a malformed declaration

**P3 · Reproduced · B012**

`<! doctype html><div></div>` is accepted. The doctype parser splits the body on whitespace and drops empty fields, so it accepts a space immediately after `<!`. The HTML tokenizer only recognizes the keyword at the markup-declaration position. [Doctype parser](../src/html.rs#L228-L249) · [HTML markup declaration open state](https://html.spec.whatwg.org/multipage/parsing.html#markup-declaration-open-state)

**Fix and acceptance.** In the strict bootstrap path, require the case-insensitive keyword immediately after the opener and valid whitespace before the HTML name. Test mixed case and allowed whitespace in valid declarations and reject the malformed form explicitly. Carry this input into B013's standards-based recovery tests when it replaces the strict path.

### HTML-04 The attribute cap is missing from the public resource contract

**P3 · Code review · B012**

The parser caps each start tag at 128 attribute tokens including duplicates. The code tests the boundary and keeps the first duplicate, but the public resource table omits the cap. Fixture authors cannot predict the rejection from the documented limits. [Limit and count](../src/html.rs#L267-L328) · [Boundary tests](../src/html.rs#L864-L877) · [Resource contract](M1_SCOPE.md#resource-contract)

**Fix and acceptance.** Document this as a bootstrap guard, including its counting rule. Keep any formal full-M1 contract change under versioned review. Verify acceptance at 128 and rejection at 129 for both distinct and repeated attributes under the documented policy.

## Evidence and contract findings

### EVID-01 The external stylesheet oracle accepts unrelated failures

**P1 · Reproduced with an isolated harness check · B011 and B018**

M1-B005 has no required diagnostic in the manifest. The rejection executor falls back to an empty substring, so any positive exit that leaves the output paths absent satisfies the check. A simulated exit 1 with a missing-fixture diagnostic passed the oracle. M1-B004 tests only the generic word `unsupported`. The real audited run emitted the intended stylesheet and text diagnostics. [Rejection executor](../tools/run_m1_bootstrap.py#L135-L147) · [Frozen negative cases](../planning/m1-cohort.json#L445-L472)

**Fix and acceptance.** Require a specific failure category for each negative case and match it to a stable CLI diagnostic or code. Assert the same fixture succeeds when the unsupported feature is removed. Wrong-feature, argument, fixture-read, panic and success outcomes must all fail the negative oracle.

### EVID-02 Frozen coverage can shrink without a contract revision

**P1 · Reproduced with a temporary manifest copy · B010 and B018**

An M1-v1 test copy with one case and zero asset entries passed the contract verifier. The runner checks the cohort name, uniqueness, a nonempty bootstrap subset, paths and the hashes of assets still listed. It derives the reported denominator from whatever remains. The recorded hash identifies the altered manifest but the runner has no independently reviewed identity to compare it against. [Contract verification](../tools/run_m1_bootstrap.py#L40-L56) · [Selection and reporting](../tools/run_m1_bootstrap.py#L187-L221) · [Frozen evidence rules](../planning/m1-cohort.json#L1136-L1140)

**Fix and acceptance.** Require a versioned, reviewed manifest identity and the exact case and asset roster for that version. Validate strict types, reject duplicate JSON keys, and require hash coverage for every referenced immutable input. CI must reject a removed case, changed stage, missing hash coverage, or a semantic edit under the same revision. Process the C001 correction as a visible revision.

### EVID-03 Source and compiler identity are independent of the tested binary

**P1 · Code review · B011, B018 and B019**

The runner accepts an arbitrary `--binary`, takes the current checkout's commit and dirty flag, samples a compiler, and hashes current source files after the cases run. The executable checksum identifies what was replayed. The surrounding source and compiler fields cannot establish which compiler, source inputs or flags produced that executable. [Git and compiler identity](../tools/run_m1_bootstrap.py#L109-L132) · [Binary selection](../tools/run_m1_bootstrap.py#L174-L193) · [Report identity fields](../tools/run_m1_bootstrap.py#L202-L220)

**Fix and acceptance.** Bind evidence to a verified build record containing the actual executable path and digest, build inputs, compiler, target and flags. Record or check source and input identities across the run. A trusted build record must match the tested executable. A stale, swapped or independently supplied executable without that record may run as exploratory evidence and must be labeled accordingly.

### EVID-04 Reruns replace existing evidence

**P1 · Reproduced · B018 and B019**

Two replays to the same temporary destination both succeeded and replaced the report and its command log. The normal Make target also reuses the default path. The runner writes the sidecar first and then the report, leaving a mismatch possible if the second write fails. The project evidence policy requires a new run for changed inputs and preservation of prior evidence. [Default and output writes](../tools/run_m1_bootstrap.py#L174-L226) · [Make target](../Makefile#L14-L19) · [Evidence identity policy](VALIDATION.md#evidence-identity)

**Fix and acceptance.** Publish a complete bundle under a unique run identity. Stage both files and publish them only after validation. An existing destination must remain intact when a retry occurs or publication fails. Verify the sidecar checksum and confirm that two completed runs have separate discoverable bundles.

### EVID-05 Timeouts and harness failures lose per-attempt evidence

**P2 · Reproduced and code review · B018 and B019**

A simulated timeout left zero command records because recording occurs after the subprocess call returns. Cases that hit the catch block receive generic `fail` outcomes, and some malformed JSON shapes can raise uncaught type errors. Temporary DOM, image, and layout artifacts disappear after an assertion failure, and CI archives only the JSON report and log. [Subprocess recording](../tools/run_m1_bootstrap.py#L93-L106) · [Case execution and cleanup](../tools/run_m1_bootstrap.py#L135-L171) · [Case outcomes](../tools/run_m1_bootstrap.py#L194-L201) · [CI artifact selection](../.github/workflows/plan.yml#L45-L55)

**Fix and acceptance.** Record every attempted invocation with duration, outcome, partial diagnostics and repetition index. Distinguish test failure, timeout, crash and harness error. Retain the artifacts available at failure with their hashes. Verify the affected case stays in the denominator and other cases continue for each injected failure type.

### EVID-06 The render oracle accepts inconsistent structured output

**P2 · Reproduced with synthetic JSON · B018**

The oracle collapsed contradictory duplicate boxes for a node into one record and accepted Boolean values in integer geometry fields. Equality checks on selected rectangles and pixels assume the DOM and layout have consistent schemas. [Render oracle](../tools/run_m1_bootstrap.py#L66-L90)

**Fix and acceptance.** Check typed fields and required key sets, unique node and box identities, and defined references between DOM and layout before checking independent fixture assertions. Duplicate boxes, Boolean integers, missing keys, invalid references and altered expected pixels must fail a targeted harness test.

### EVID-07 The Make target can replay a different executable from the one it builds

**P2 · Confirmed by Make dry run · B011 and B018**

With `CARGO_TARGET_DIR` set, `make -n engine-check` still passes `target/debug/browser-engine` to the verifier. Cargo can build to the configured location while this target replays an older default binary or fails because that path is absent. [Make target](../Makefile#L14-L19)

**Fix and acceptance.** Use the executable path emitted by Cargo or use one explicit target directory consistently for build and replay. Verify a run with a nondefault target directory checks that binary even when an older default binary exists. Include its path and digest in the EVID-03 build record.

### EVID-08 Local handoff files can change the repository validation result

**P3 · Reproduced in a temporary checkout fixture · Repository validation**

The Markdown scan walks all eligible filesystem paths, including the Git-excluded local handoff. A broken link in a local-only handoff document can fail local validation even though clean CI never sees the file. The validator checks local file targets and gives no guarantee about the referenced headings. [Markdown traversal](../tools/check_plan.py#L207-L214) · [Link verification](../tools/check_plan.py#L216-L252)

**Fix and acceptance.** Exclude explicitly local handoff material from the repository scan or enumerate intended repository documents with an option for new files. The same repository documents must receive the same result in the developer checkout and clean CI. Keep the documented coverage limited to file targets unless anchor checking gets its own implementation and tests.

### CONTRACT-01 The C001 expected background contradicts CSS specificity

**P1 · Known frozen-oracle discrepancy verified against source and CSS · B010 and B014**

`main > div.item` and `div + div` both match `#b` at equal origin and importance. The red rule has specificity `(0, 1, 2)` and beats the green rule's `(0, 0, 2)`. The manifest expects green. The current capability documentation already flags the discrepancy. [Fixture](../fixtures/m1/full/selectors.html) · [Expected C001 values](../planning/m1-cohort.json#L595-L620) · [Recorded discrepancy](M1_SCOPE.md#m1-c001-specificity-discrepancy) · [Selector specificity](https://www.w3.org/TR/2018/REC-selectors-3-20181106/#specificity) · [Cascade sorting order](https://www.w3.org/TR/2022/CR-css-cascade-5-20220113/#cascade-sort)

**Fix and acceptance.** Record a reviewed cohort revision with a red expectation for the conflicted element and a separate assertion where the adjacent selector wins. Keep the same coverage and identify any results invalidated by the revision. Correct selector and cascade code should pass both assertions.

## CLI and regression coverage findings

### CLI-01 Output cleanup failures are hidden

**P2 · Code review with rollback control · B018**

The CLI stages files and publishes them one at a time. It discards the result of every cleanup or rollback removal. If cleanup itself fails, the command may return success with an extra staging file or report only the original error while a partial output remains. A normal case-folded collision on the audit host exercised the second-publication failure and successfully removed the first output and both staging files. The existing collision test stops before staging. [Publish and rollback](../src/main.rs#L144-L203) · [Existing collision test](../tests/cli.rs#L111-L146)

**Fix and acceptance.** Preserve the original failure and surface unexpected cleanup errors with the affected paths. Define what callers can observe when a paired write fails. Test an injected failure after the first publication, preserving preexisting files and the fixture. Verify clean rollback on the ordinary failure path and a precise diagnostic when removal itself fails.

### CLI-02 Some integration assertions miss the behavior named by the test

**P3 · Code review · B018**

The zero-width case supplies neither required file option. It fails before dimension parsing. The over-limit check asserts generic failure, so it also gives limited coverage of the actual dimension error. The success test scans three-byte windows anywhere in the file and accepts any nonempty layout text. This is weaker than the separate frozen geometry and pixel checks. [Argument validation order](../src/main.rs#L75-L126) · [CLI output assertions](../tests/cli.rs#L67-L108) · [Over-limit test](../tests/cli.rs#L166-L195)

**Fix and acceptance.** Exercise width and height with an otherwise valid command and assert the exact relevant diagnostic and absence of both outputs. Cover zero, one, and the values just below, at and above the limit. Parse the delivered JSON and assert a known rectangle and a red pixel at a known PPM coordinate.

### TEST-01 The mutation smoke test ties every seed to one edit type

**P3 · Reproduced from the test generator · B019**

Both seed selection and mutation choice use `index % 3`. The three seeds receive deletions, insertions, and replacements respectively, with zero coverage of the other combinations. The 4,096 iterations yield 2,586 distinct strings on the 64-bit audit host. The test mutates ASCII only and can pass without asserting a minimum number of successful render comparisons. [Mutation smoke test](../tests/robustness.rs#L7-L35)

**Fix and acceptance.** Exercise all edit types for each seed and add deterministic Unicode inputs that preserve valid UTF-8. Record the number of parse and render successes that reached the equality assertions. Keep the smoke test repeatable and track the sustained campaign as a separate B019 gate.

## Offline broker experiment findings

### IPC-01 The replay has no local deadline for child I/O or exit

**P2 · Code review · Offline example and B020**

The parent performs blocking reads, writes and waits sequentially. Cleanup in `Drop` only runs when the call returns or unwinds. A regression in the synthetic child can leave a local replay waiting indefinitely, and CI can only recover at its job timeout. The current two-child normal replay completed. [Frame I/O](../examples/m2_broker.rs#L42-L69) · [Exchange, wait and cleanup](../examples/m2_broker.rs#L115-L144) · [Job timeout](../.github/workflows/plan.yml#L19-L25)

**Fix and acceptance.** Bound the example replay with a deadline and deterministic child cleanup. Test a normal completion and a controlled lifecycle failure with a clear timeout outcome and both fixture processes reaped. Design production peer lifecycle controls separately in B020 after B019.

### IPC-02 Parent and fixture share the expected response encoder

**P3 · Code review · Offline example and B020**

Both the parent and the fixture child use `response()` to construct grant and denial payloads. A response layout change could affect both sides identically and leave the replay green. The tests have a literal request-wire oracle and no corresponding independently authored response bytes. [Response encoder](../examples/m2_broker.rs#L93-L100) · [Parent and child exchange](../examples/m2_broker.rs#L115-L162) · [Request wire test](../examples/m2_broker.rs#L213-L226)

**Fix and acceptance.** Assert independent fixed bytes for the complete success and denial frames, including the header, request ID, status and resource bytes. Changing one response field or the encoding order must fail even when parent and fixture use the same build.

## Remaining M1 work

**6 of 23 frozen full-M1 cases (26%) matched every stated assertion in direct checks.** The percentage gives each case equal weight and describes the fixture set. On macOS arm64, the checks compared temporary CLI outputs, diagnostics, and generated boundary inputs with the frozen expectations. Cases with partial matches or missing engine entry points contribute zero to this count. The official runner has recorded **5 of 5 bootstrap passes**, and all **23 full-M1 cases** still await an official run. The roadmap marks **2 of 10 M1 tasks complete**, covering the scope freeze and reproducible bootstrap. [Frozen manifest](../planning/m1-cohort.json) · [Bootstrap evidence](../evidence/M1/BOOTSTRAP.md) · [M1 roadmap tasks](../planning/roadmap.json#L160-L332)

| Frozen group | Direct matches | Roadmap | Observed behavior and remaining work |
| --- | --- | --- | --- |
| H001–H006 HTML and DOM | 1 of 6 · H004 | B012, B013 | H004 decoded the exact paragraph text. H001 lacked the doctype node, H002 lacked a `div` fragment context, and H003 rejected omitted paragraph end tags. H005 retained the first duplicate attribute and H006 preserved raw style text, but both lacked the generated body. Add document-owned mutations with stable IDs, cycle and cross-document checks, and bounded teardown. [HTML cases](../planning/m1-cohort.json#L475-L595) |
| C001–C003 style | 0 of 3 | B014 | C003 matched the 10×12 red box. The current interface lacks computed-style output for the expected text color and the full C001/C002 assertions. Resolve CONTRACT-01, finish syntax recovery, and expose computed values, UA and user origins, inheritance, and initial values. [Style cases](../planning/m1-cohort.json#L596-L664) |
| L001–L002 layout | 1 of 2 · L002 | B016 | Both L002 negative-margin rectangles matched. L001 still needs inline whitespace collapse, line formation, and wrapping. [Layout cases](../planning/m1-cohort.json#L665-L725) |
| T001–T005 shaping and text | 0 of 5 | B015–B017 | The pinned references are available. Implement engine shaping for Latin, Arabic, and Devanagari and compare glyph IDs, clusters, positions, and direction. Integrate Arabic and Devanagari layout and paint with independent output checks. [Text cases](../planning/m1-cohort.json#L727-L821) |
| P001–P003 paint and resources | 1 of 3 · P001 | B017 | All five P001 clipping pixels matched. Add controlled PNG decoding and alpha compositing, then exercise truncated input rejection and decoded-size bounds. [Paint cases](../planning/m1-cohort.json#L823-L939) |
| R001–R004 resource boundaries | 3 of 4 · R001, R003, R004 | B018, B019 | Input byte size, element depth, and viewport width matched their below, at, and above-limit expectations. R002 accepted the fixture that should contain 10,001 nodes because the parser omits its doctype node and counts 10,000. Count the doctype and generated nodes, then replay all boundary cases through the official adapter. [Boundary cases](../planning/m1-cohort.json#L942-L1043) |
| **Total** | **6 of 23** | **B012–B019** | **Direct checks against frozen case assertions.** |

B018 needs execution and assertion paths for the full contract. The current helper ran the engine successfully on P001, then failed because it requires a boxes oracle and P001 specifies pixels. The full adapter needs to handle both case shapes. [Runner dispatch](../tools/run_m1_bootstrap.py#L135-L194) · [B018 contract](../planning/roadmap.json#L303-L316)

Before enabling visible text in the default engine path, define how the bootstrap-only B004 rejection is preserved as historical evidence or revised under a new contract. For B019, enforce the frozen 512 MiB RSS and 5-second fixture limits in isolation, record 100 repetitions of every required fixture, add same-process replay, replay on a second equivalent worker, and complete the seven-day coverage-guided campaign. Linux and macOS development CI results are separate platform checks. [Resource limits](M1_SCOPE.md#resource-contract) · [Determinism and fuzz requirements](VALIDATION.md#determinism-bounds-and-adversarial-runs) · [B019 contract](../planning/roadmap.json#L319-L332)

M2 implementation and acceptance remain gated by B019. Freeze the M2 candidate and Linux evaluation baseline before accepting B020. Build containment and policy after B020, integrate B023, then obtain the independent B024 review. B030 VM build and embedding feasibility can proceed in isolation alongside M1 work. Desktop app, accessibility, updates and release work remain in their roadmap stages. [M2 implementation order](M2_SCOPE.md#implementation-order) · [Roadmap](ROADMAP.md)

## Fix order

| Order | Workstream | Findings and completion check |
| --- | --- | --- |
| 1 | Make bootstrap results trustworthy | EVID-01, EVID-02, EVID-03, EVID-04 and EVID-07. A fresh report should identify the exact reviewed cohort and freshly built executable, distinguish intended rejections, and preserve the previous run. |
| 2 | Correct the current renderer | Start with CSS-01, CSS-02 and CSS-03 together so recognized unsupported features and malformed values follow one policy. Follow with CSS-04 through CSS-06 and RENDER-01. Add a small independent pixel or geometry regression for each behavior. |
| 3 | Complete parser and DOM work | Address HTML-01 through HTML-04 while delivering B012 and B013. Keep tokenization separate enough for tree-construction state to drive document, fragment, and raw-text modes. Verify stable node identity and generated-node limits. |
| 4 | Finish acceptance tooling and contract work | Resolve CONTRACT-01 through a reviewed revision. Implement EVID-05, EVID-06, CLI-01, CLI-02, TEST-01 and the full case adapters while their engine features land. EVID-08 is independent maintenance work. |
| 5 | Close full M1 | Complete B015–B018, measure RENDER-02, and satisfy B019's isolated limits, worker replay and fuzz evidence. Update the capability matrix only after recorded results. |
| 6 | Advance the broker and Linux boundary | Fix IPC-01 and IPC-02 in the offline experiment. Keep the reviewed B019, B020, B021/B022, B023 and B024 dependency sequence for product integration. |
