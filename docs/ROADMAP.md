# Delivery roadmap

Planning baseline: 16 November 2026; implementation update: 20 November 2026. M0 is complete and M1 has a working controlled-fixture rendering bootstrap. No engine milestone is complete, no browser compatibility score has been measured, and no date below is a release commitment. See the [current capability matrix](../evidence/M1/capability-matrix.md) for implemented behavior.

## Product strategy

BroWSER will own its HTML tree construction, DOM, CSS cascade, layout, display list, and rendering orchestration. Rust is the default for first-party implementation. Maintained libraries may supply cryptography, text shaping, fonts, codecs, and a JavaScript VM; a custom engine does not require reimplementing every dependency. The first product target is Linux x86_64, beginning with an offline headless renderer. macOS supports development; Windows and additional architectures require separate porting gates.

The initial use case is reproducible rendering and inspection of controlled documents. That creates a useful product and a compatibility laboratory before the cost of general browsing. The competitive goal is a secure, interoperable everyday browser with demonstrable advantages in responsiveness, memory, or inspectability. Each advantage needs a named workload, a control browser, raw measurements, and a repeatable method.

Full Chromium or Firefox parity is an ongoing program. Extensions, DRM, mobile, enterprise management, browser account sync, and a new JavaScript JIT are outside the initial delivery sequence. Unsupported features remain explicitly unsupported; they do not acquire empty APIs to make feature detection succeed.

## Milestones and release authority

For M1 and later, the engineering lead owns scope and a different reviewer owns evidence approval. Security approval is a separate veto for M2 and later. Roles are responsibilities to staff, not claims that a team has been hired. Engine milestone completion requires a tagged commit, a capability matrix, the evidence listed below, and a review recording each gate as passed. M0 is a repository packaging gate: checked documents, passing repository contracts, and public publication, recorded in [BASELINE_VERIFICATION.md](BASELINE_VERIFICATION.md). Agent-assisted consistency review of M0 does not substitute for the independent human security reviews required by later gates. A blocked gate delays the milestone; lowering a threshold requires a public scope revision before the next run.

| ID | Deliverable and dependency | Required exit evidence | Explicit boundary |
| --- | --- | --- | --- |
| M0 | Reviewed project baseline; no engine prerequisite | Repository validator and its negative tests pass; architecture, threat model, resources, and delivery gates agree; repository is published with current status | Documentation and tooling only; no browser release |
| M1 | Offline headless renderer; depends on M0 | Own parser-to-pixel pipeline renders the frozen M1 cohort; 100% of cohort assertions and reftests pass; 100 consecutive identical renders produce identical pixels on the pinned environment; malformed-input suite terminates within limits; seven consecutive days of automated parser and layout fuzzing have no unresolved reproducible crash | No network, JavaScript, authenticated content, or claim of general HTML/CSS support |
| M2 | Sandboxed network browser; depends on M1 and platform security review | Navigation, cancellation, redirects, TLS errors, resource loading, history, and bounded downloads pass the frozen M2 suite; every origin, storage, IPC, and sandbox invariant passes; renderer-compromise tests cannot open files or sockets; one tab crash does not terminate another; independent security review has no open critical or high findings | Controlled browsing alpha; no arbitrary public browsing until security gate approval |
| M3 | Dynamic platform; depends on M2 and completed VM embedding decision | DOM bindings, event loop, script/module loading, GC tracing, mutation/layout, fetch, timers, and promises pass the declared cohort; Test262 adapter honors upstream execution semantics; embedded VM shows no unexplained conformance regression against its standalone baseline; cross-origin and lifecycle tests pass with scripts enabled | No claim of whole-web compatibility; missing APIs stay absent |
| M4 | Developer alpha; depends on M3 | Flex/grid, forms/input, international text, accessibility, inspector, and WebDriver coverage meet the published alpha matrix; 90% or more of a frozen 100-task site corpus succeeds; thirty-day supervised use completes without unresolved critical/high security defects or profile corruption; signed install/update/rollback drills pass | Opt-in developers; supported platform and known failures remain visible |
| M5 | Competitive readiness; depends on M4 and sustained operations | At least 99% pass on the frozen release WPT scope, with per-directory and full-suite counts; 98% or more of a frozen 500-task corpus succeeds; no open critical/high security defects; independent sandbox/VM/update review passes; 90-day update and incident drill passes; published performance and accessibility gates below pass | General availability only for platforms passing every gate; no universal compatibility guarantee |

M2 and M3 are controlled internal test builds. External distribution of any network-capable build also requires the applicable B042 signed-update, private-reporting, patch-response and recovery gates, even if distribution is proposed before M4. Milestone numbering never waives an operational security requirement. Network tests may load remote content only after their containment gate passes.

M1 cohort requirements are: UTF-8 input; HTML document and fragment parsing; malformed nesting and character references; DOM text/element/attribute handling; CSS tokenization; type/class/ID and combinator selectors; origin, specificity, and source-order cascade; box sizing and block/inline flow; margins/padding/borders; whitespace and line wrapping; shaped Latin, Arabic, and Devanagari text using pinned fonts; clipping, colors, PNG decoding, and deterministic raster output. The cohort includes positive, negative, boundary-size, and malformed cases for each item. Freeze exact test IDs, expected outputs, fonts, limits, and upstream revisions before M1 implementation is reviewed. The cohort is a product scope, not a substitute for publishing out-of-scope failures.

M2 adds URL parsing/canonicalization, HTTPS and HTTP policy, certificate verification, redirects, fetch cancellation, content limits, content-type handling, cookies, cache partitioning, navigation commits, same-origin enforcement, cross-origin requests, CSP enforcement for supported resource types, renderer process selection, and broker authorization. Every feature needs adversarial tests before exposure to remote content.

M3 adds script execution ordering, Web IDL conversion, realms, exception propagation, microtask checkpoints, DOM events, module resolution, GC stress, document teardown, stale callbacks, navigation races, and supported web APIs. VM conformance and browser embedding correctness are different evidence streams. A passing standalone VM cannot certify DOM security.

## Performance and compatibility gates

Before M4, freeze the site-task corpus, supported features, reference hardware, and exact Chromium and Firefox versions. Tasks cover reading, search, navigation, forms, editing, and representative application interactions, using licensed reproducible captures or controlled test sites. At M5, require median task latency no worse than 1.25 times the faster control browser and peak process-tree memory no worse than 1.25 times the lower-memory control on at least 90% of tasks. Require at least one declared product advantage of 15% or more on its predeclared workload, with a confidence interval excluding zero improvement. These are chosen project targets, not measured results or forecasts.

The release WPT scope includes every test for each advertised feature, plus security tests regardless of whether a feature is advertised. Publish the complete pinned-suite denominator and counts of not-run, unsupported, timeout, crash, and failure outcomes alongside the scoped percentage. A selected-cohort score cannot be called whole-web compatibility. The 100-task and 500-task corpora are different gates; freeze both before measuring their corresponding milestones.

Accessibility is a release requirement: keyboard-only completion of every supported UI task, focus order and visible focus checks, screen-reader completion of the critical task set on the supported platform, and no critical accessibility finding. Automated checks complement human testing. Rendering optimization cannot remove accessible semantics or weaken isolation.

## First 90 days

These relative days begin when the team and CI capacity are funded. They assume eight full-time engineers with relevant systems experience and access to security, design, and accessibility reviewers. Their purpose is to retire risks, not promise M1 within one quarter.

| Period | Work and owner role | Review artifact and continuation decision |
| --- | --- | --- |
| Days 1–14 | Lead and test lead freeze M1 specification/test cohort, toolchain, dependency policy, and reference environment; platform engineer prototypes renderer/broker IPC; VM engineer evaluates embedding | Reviewed capability matrix, actual build/run evidence, and risk register. Stop expanding scope if independent builds or ownership rules cannot be reproduced |
| Days 15–30 | Parser/DOM and style engineers implement end-to-end tree and cascade paths; rendering engineer establishes text and raster pipeline; security engineer defines sandbox denial tests | Render one document through every real stage. Record missing capabilities explicitly. Choose VM based on buildability, GC integration, licensing, update burden, and representative binding tests |
| Days 31–60 | Layout/text team completes M1 cohort incrementally; test engineer runs reftests, determinism tests, malformed cases, fuzzing, and resource-limit tests; platform team prototypes sandboxed navigation separately | Weekly capability/evidence report with named failing tests. Reject visual “success” if unsupported syntax was silently accepted |
| Days 61–75 | Resolve correctness and lifetime defects; profile actual bottlenecks; replay corpus on clean workers; independent architecture and unsafe-code review | M1 candidate evidence, dependency inventory, and all known failures. M2 work remains isolated until policy review passes |
| Days 76–90 | Reproduce M1 evidence on a second machine; complete fuzz observation window; review security and schedule; revise staffing and dates from observed throughput | Accept M1 only if every gate passes. Otherwise issue a revised forecast, preserve full failure evidence, and continue M1 without relabeling an incomplete result |

Critical path: pinned cohort → parser/DOM ownership → cascade and font metrics → layout → pixels → deterministic reference comparison. GC/DOM interoperability and sandbox feasibility run in parallel because either may invalidate an architectural choice. Public-network enablement depends on security evidence, not on the end of this calendar.

## Forecast and change control

For an experienced, funded team, use M1 at roughly 3–6 months, M2 at 6–12 months, M3 at 12–24 months, and M4 at 24–36 months as initial cumulative planning ranges. M5 is a multi-year program whose date cannot responsibly be forecast before M3 evidence. These ranges are internal estimates, not industry statistics. Team scale does not make every dependency parallel.

At each milestone, recalculate remaining effort from reviewed work throughput, unresolved defects, dependencies, and security findings. Maintain a public change log of capability additions, scope reductions, and forecast changes. Version the capability matrix and corpora with the release so later comparisons do not move their denominator.

## Standards and evidence sources

Primary sources checked 16 November 2026:

- [WPT running tests](https://web-platform-tests.org/running-tests/index.html): upstream runner and custom-runner entry points.
- [WPT reftests](https://web-platform-tests.org/writing-tests/reftests.html): test/reference comparison and declared fuzzy matching.
- [Test262 interpretation](https://github.com/tc39/test262/blob/main/INTERPRETING.md): realm, flags, harness, negative tests, and host behavior.
- [Speedometer 3.1](https://browserbench.org/Speedometer3.1/): application responsiveness benchmark; use only after the browser can correctly execute its workloads.

The thresholds, staffing, sequence, and forecasts in this roadmap are project decisions. The sources define tools and semantics; they do not endorse BroWSER or certify these targets.
