# Validation and evidence policy

Status updated 20 November 2026: the M1 bootstrap has local parser, renderer, CLI and fixture checks. Their scope and actual outcomes are recorded in [bootstrap evidence](../evidence/M1/BOOTSTRAP.md). No engine benchmark or WPT result is claimed. Repository integrity checks still verify the planning package separately from engine behavior and security.

## Evidence identity

Every future engine result must identify the BroWSER commit and build flags; toolchain and dependency lock revisions; operating system, CPU, RAM, GPU, driver, display scale, locale, fonts, and power mode; corpus and upstream suite commits; runner commit and configuration; start/end time; outcome; and raw artifact hashes. Keep stdout, stderr, crash diagnostics, machine-readable results, test/reference screenshots, and performance samples. A report without this identity is exploratory and cannot close a gate.

Store public sanitized evidence in an immutable release artifact or equivalent content-addressed store and link it from the reviewed milestone record. Retain passing release evidence for the supported lifetime plus one year. Retain minimized regressions in version control. A changed binary, corpus, reference, font, or harness creates a new run; it cannot overwrite the old result.

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
