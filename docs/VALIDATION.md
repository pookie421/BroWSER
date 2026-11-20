# Validation and evidence policy

Updated 21 November 2026. The offline engine has local bootstrap and full-M1
fixture reports, same-process replay tests and short coverage-guided runs.
[M1 capability matrix](../evidence/M1/capability-matrix.md)

## Evidence identity

Every future engine result must identify the BroWSER commit and build flags; toolchain and dependency lock revisions; operating system, CPU, RAM, GPU, driver, display scale, locale, fonts, and power mode; corpus and upstream suite commits; runner commit and configuration; start/end time; outcome; and raw artifact hashes. Keep stdout, stderr, crash diagnostics, machine-readable results, test/reference screenshots, and performance samples. A report without this identity is exploratory and cannot close a gate.

Store public sanitized evidence in an immutable release artifact or equivalent content-addressed store and link it from the reviewed milestone record. Retain passing release evidence for the supported lifetime plus one year. Retain minimized regressions in version control. A changed binary, corpus, reference, font, or harness creates a new run; it cannot overwrite the old result.

Run `python3 tools/run_m1_bootstrap.py --build --repeat 100` for development
bootstrap evidence. The runner pins the cohort identity, builds with the locked
offline Cargo inputs, records the executable Cargo produced, and verifies build,
source and cohort hashes across the replay. It publishes the result and command
log together in a fresh `artifacts/m1/bootstrap-*` directory, with per-attempt
timing, outcome and diagnostics and any outputs retained from failing cases.
The `--binary` option provides an exploratory replay of an existing executable.
Development evidence covers the bootstrap, with the full-M1 and WPT denominators
reported separately. The repository Markdown check validates file targets and
excludes unpublished local handoff files.

Run `python3 tools/run_m1_bootstrap.py --stage full-m1 --repeat 100` for the
23 full-M1 cases. It exercises the engine commands with typed assertions over
DOM trees, computed styles, glyph runs, laid-out text, images, and generated
limits. A text render can match its glyph reference and retain a screenshot
while independent raster review is pending. Only reviewed passing cases count
toward the M1 gate.

Full-M1 runs default to a five-second fixture deadline and a 512 MiB RSS
monitor. Each fixture process gets a fresh scratch home and cache and a
fixed locale. The supervisor samples the child every 10 ms, terminates its
process group on a detected overage, and checks the kernel's per-child peak
when it reaps the process. A final peak above the cap is a resource failure
even if the fixture returned zero. The command log records the measured
values, limit trigger and any partial output. An unavailable requested
backend fails before fixture execution. [Fixture supervisor](../tools/m1_limits.py)

The supervisor's sampling can permit a brief overshoot before termination.
It monitors the direct fixture child. OS filesystem and network isolation
and any process-tree hard memory limit require separate worker controls.
Bootstrap can opt into the same checks with `--rss-policy sampled`.

Use `--worker-id` with a non-private operator-assigned label when recording
an equivalent-worker replay. On each provisioned worker, retain the build-bound
report, command log and case artifacts. Compare the two complete bundles with
`python3 tools/compare_m1_workers.py FIRST/full-m1-report.json SECOND/full-m1-report.json`.
The comparison validates artifacts, repetition logs, matching source and
build settings, recorded platform metadata and exact fixture hashes. It
preserves the declared worker labels and outstanding raster reviews. The
reviewer verifies that the labels correspond to independently provisioned
equivalent workers. [Worker comparison tool](../tools/compare_m1_workers.py)

The CLI stages output bytes and publishes each file without overwriting
existing paths. A paired render can briefly expose its first file during
publication. On failure the CLI rolls back files it published and reports
any cleanup error with the affected path.

## Testing layers

| Layer | Required behavior and evidence | Cadence after implementation |
| --- | --- | --- |
| Parser and value semantics | Golden token/tree cases, malformed recovery, round-trip properties where valid, length/overflow limits | Every relevant change |
| DOM and lifetime | Ownership invariants, mutation, adoption, document teardown, GC stress, stale callbacks, event dispatch reentrancy | Every relevant change; sanitizer/GC-stress jobs nightly |
| Style and layout | Cascade assertions, geometry assertions, writing modes, text shaping, fragmentation as introduced, reference pixels | Every relevant change |
| Network and origin | Controlled servers exercise redirects, certificates, CORS, cookies, partitioning, CSP, cancellation, navigation races | Every change to privileged paths |
| Sandbox and IPC | Compromised-child tests attempt denied file/socket/device operations; malformed and oversized IPC; stale capability reuse; cross-tab access | Every broker/protocol change and each supported OS release |
| End-to-end | Versioned site tasks, input, accessibility tree, keyboard and screen-reader workflows, crash recovery, profile integrity | Nightly and release candidate |
| Robustness | Coverage-guided fuzzing of parsers, CSS, URLs, image/font boundaries, IPC, DOM mutations; minimized reproducible corpus | Continuous; seven clean days required at M1 and repeated after major parser changes |
| Supply chain and updates | Dependency inventory, license review, advisory triage, reproducible build checks, signed manifest and downgrade/rollback tests | Every dependency/update change and release |

Passing the same test under the same assumptions twice does not create independent evidence. Test changes need review, especially changed references, widened tolerances, disabled cases, and lowered timeouts. Use independent implementations/specifications as oracles where meaningful; do not make expected output by copying current engine output and calling it correct.

## Web Platform Tests

WPT is the shared browser interoperability test suite; its documentation describes running and custom runners. BroWSER requires a real product adapter or WebDriver integration before claiming upstream harness execution. Until then, extracted offline cases are labeled derived tests and retain upstream origin, revision, and license. A custom screenshot utility is not a full WPT runner. [WPT running tests](https://web-platform-tests.org/running-tests/index.html)

Before a milestone, commit an exact list of selected test URLs and variants, the upstream WPT commit, expected supported features, and known exclusions. Include all relevant tests of an advertised behavior rather than choosing only passing examples. Report both files and assertions/subtests. For the selected cohort, the denominator is all applicable assertions and references fixed before the run. Failures, timeouts, harness errors, and crashes remain in that denominator. Skipped or not-run cases do not pass. Report the full pinned manifest separately so readers can see how much of the platform remains uncovered.

Keep these outcome counts distinct: pass, fail, timeout, crash, harness error, expected failure, unsupported/not run, and manual test awaiting review. An expected failure remains a failure for capability and pass-rate claims; expectation metadata only controls regression alerts. Run representative stable Chromium and Firefox versions against the same upstream revision and compatible environment when presenting comparisons. Publish their actual version strings and raw results.

WPT reftests compare a test document to declared references. Use pinned fonts, viewport, device scale, color configuration, animation state, and completion conditions. Honor upstream match/mismatch and fuzzy metadata. Any additional tolerance requires a reviewed reason and bounds; a global screenshot similarity threshold can hide a broken layout. Retain both screenshots and pixel diff. [WPT reftest semantics](https://web-platform-tests.org/writing-tests/reftests.html)

Security tests are mandatory even when they exercise unsupported features or error paths. Standards pass rates cannot certify sandbox strength, exploitation resistance, or profile confidentiality. The milestone reviewer also needs the adversarial evidence in the security model.

## JavaScript and Test262

Pin the Test262 commit and interpretation version together with the VM build. Run the standalone VM as a baseline and the embedded engine with a host adapter. Use upstream metadata for flags, negative phase/error type, includes, async completion, modules, and feature handling. Each test receives a fresh realm. Strict/non-strict variants count as distinct executions where required. Host helpers must implement the required semantics; missing helpers produce unsupported or harness-error outcomes, never a manufactured pass. [Test262 interpretation](https://github.com/tc39/test262/blob/main/INTERPRETING.md)

Report test files, execution variants, selected feature scope, and full-suite counts independently. An unexplained regression versus the same VM standalone build blocks M3. Document justified embedding differences with minimized cases and review. Keep DOM/Web IDL, task/microtask ordering, origin enforcement, and browser module-loading tests separate because Test262 alone does not validate browser bindings. The suite itself aims at broad ECMAScript coverage and is not a completeness proof. [Official Test262 project](https://github.com/tc39/test262)

## Determinism, bounds, and adversarial runs

For M1, repeat every frozen fixture 100 times on the same environment and compare exact output hashes and pixels. Reset process state between fixtures; separately run multiple documents in one process to detect leaked state. Record output stability across a second equivalent worker. Platform-dependent text rasterization is reported separately; do not claim cross-platform pixel identity without evidence.

Before the first implementation gate, choose and version explicit limits for input bytes, node count, nesting depth, decoded resource bytes, page memory, IPC size, and execution time. Tests cover just below, at, and above each limit; rejection must be deterministic, diagnostic, and bounded. Treat limits as product behavior and publish them. The security review chooses limits from measured behavior and threat model rather than an arbitrary number copied into an untested plan.

Fuzz campaign records include harness, seed corpus, coverage instrumentation, CPU time, executions, discovered unique failures, minimized cases, fixes, and post-fix reruns. A seven-day observation window does not mean a program is safe; it supplies one bounded gate. Reset the window for affected targets after a substantial lifetime or parser change. Any unresolved reproducible memory safety defect, escape, cross-origin leak, or profile corruption blocks release.

The parser and renderer have checksum-locked libFuzzer targets with
AddressSanitizer. The campaign driver freezes the source and seed corpus,
runs both targets concurrently, and retains telemetry, artifacts and any
failure. It records the measured overlap and keeps the seven-day check open
until both targets complete with at least 604,800 seconds of observed overlap.
[Fuzz targets and reproduction commands](../fuzz/README.md)

## Performance method

Measure correctness first. A task that fails, omits content, skips work, disables security, or times out is a failed task rather than a fast sample. Freeze a task definition, corpus, hardware setup, control versions, and analysis method before comparing builds.

For each task, collect at least 30 independent measured samples after five separately recorded warm-up runs. Reset profile/cache according to a declared cold or warm scenario. Alternate or randomize browser order; keep background work, power, viewport, and network shaping controlled. Publish raw samples, median, p95, and bootstrap 95% confidence intervals. Report results per task before any aggregate; report geometric means only for valid positive ratios. Distinguish measurement noise from a regression with confidence intervals and repeat on a second worker before declaring an advantage.

Measure startup, navigation-to-usable-content, input-to-presented-frame latency, scrolling frame time, total process-tree peak memory, CPU time, energy where instrumented, and shutdown/recovery. Report cold and warm cache separately. Include network, renderer, GPU, and broker processes in memory; do not compare a renderer alone against a full browser. Keep security policies enabled for competitive claims.

Speedometer 3.1 is a responsiveness workload for web applications. Pin its source/version and follow its viewport and settings instructions. Run the full applicable workload only after BroWSER executes it correctly; an incomplete run has no valid score. It is one benchmark, not a substitute for real site tasks, accessibility, energy, or memory evidence. [Speedometer 3.1](https://browserbench.org/Speedometer3.1/)

Proposed M5 thresholds are in [ROADMAP.md](ROADMAP.md). A performance regression above 5% with a confidence interval excluding zero improvement requires owner review; a milestone threshold violation blocks that milestone regardless of average score. An approved correctness or security fix may deliberately trade performance, with published rationale and new baseline.

## Triage and gate decisions

Every failing gate records the reproducer, owning role, severity, affected capabilities, evidence location, and next review. Reproduce before classifying flakiness. Track recurring flakes as reliability defects; temporarily quarantined tests still appear in public outcome counts. Do not silently retry until green.

Release approval requires all mandatory gates passing, reviewed exclusions for optional scope, no unresolved critical/high security finding, and a clean replay from a fresh checkout on the supported platform. The milestone record states what was actually measured. Planned tests are not evidence, and this document does not imply these future runners have been implemented.

All primary sources above were checked on 16 November 2026. Numbers and thresholds are BroWSER project policy rather than claims made by upstream projects.
