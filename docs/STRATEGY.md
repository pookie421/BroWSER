# Product strategy

Planning baseline: 16 November 2026. Decisions here define project scope; measured
evidence is required before advertising capability or changing a release status.

## Product and audience

BroWSER will be an independently implemented web rendering engine and a desktop
browser that embeds it. The rendering core will not delegate page rendering to
Blink, Gecko, WebKit, or an operating-system web view. An initial headless renderer
provides a reproducible development surface before browser chrome is introduced.

The first users are engine developers and standards contributors working with
controlled fixtures. The next users are developers testing a deliberately documented
subset of the web platform. General desktop users become the audience only after
security review, accessible interaction, update delivery, and compatibility gates.

The proposed distinction is predictable resource use with explainable behavior:
bounded work queues, inspectable layout and invalidation, reproducible test runs,
and public compatibility evidence. These are hypotheses to test, not features or
advantages achieved by this repository. Avoid promising lower memory or faster
browsing until matched measurements support the statement.

## Scope decisions

| Area | Initial delivery | Expansion gate |
| --- | --- | --- |
| Platform | Linux x86_64 headless, with macOS as a development host | Linux desktop alpha; Windows and other architectures after platform owners and isolation tests exist |
| HTML | Named, frozen tokenizer and tree-construction cohorts, including malformed input | Broaden toward complete HTML parsing using pinned upstream fixtures |
| CSS/layout | Explicit block/inline subset, cascade, box model, deterministic text and paint | Flexbox, grid, positioning, writing modes, fragmentation and advanced effects by tested cohorts |
| Scripting | Disabled in offline renderer; VM feasibility measured separately | Real VM/DOM/event-loop integration in M3, with Web IDL conversion and GC tests |
| Network | No network in M1; fixture resources selected by a constrained manifest | Brokered HTTPS with URL, Fetch, origins, credentials, cache and certificate tests in M2 |
| Browser interface | Headless artifact generation | Trusted address bar, tabs, navigation, input and accessibility before developer alpha |
| Media/GPU | CPU raster baseline with controlled assets | Reviewed codec and GPU services, capability-specific tests and sandbox policies |
| Extensions/sync | Outside the initial roadmap | Separate proposals after stable security and platform contracts |

A deliberately limited implementation must behave according to the applicable
specification within its declared scope. CSS unknown declarations use specified
parsing and cascade behavior; unsupported privileged APIs must not masquerade as
successful implementations. Fixture harnesses fail explicitly when asked to test a
capability outside their supported contract. Scope reduction never changes a test
failure into a pass.

## Meaning of competitive readiness

M5 requires a release review covering all of the following, not a single benchmark:

1. Compatibility: predeclared WPT cohorts, whole-suite coverage accounting, real-site
   task success, and an explicit catalogue of unsupported APIs.
2. Security: reviewed process isolation, origin and permission enforcement, malicious
   IPC tests, fuzzing triage, and a functioning patched-release mechanism.
3. User experience: accessible navigation and forms, editing, international text,
   media, printing, recovery, developer tooling, and platform integration.
4. Performance: controlled comparisons with then-current stable Chromium and Firefox,
   reporting versions, hardware, workloads, uncertainty and regressions.
5. Operations: dependency updates, crash response, ownership coverage and funding for
   maintenance. A fast development build alone is insufficient evidence.

Targets and tolerances are proposed in [VALIDATION.md](VALIDATION.md). They are
project release criteria, not scores currently achieved or industry certification.

## Decision cadence

At the start of each milestone, the engineering lead freezes the scope, upstream
revisions, evidence format, assigned people, capacity and resource limits in a reviewed
change. Each week reviews focus on failing tests, risks and the next complete slice.
Every four weeks, compare delivered evidence with estimates and update the remaining
schedule. A milestone passes only when its acceptance evidence is reproducible at the
reviewed commit; otherwise reduce future scope or extend time transparently.

The first investment decision is whether the M1 renderer and VM/sandbox feasibility
work justify a funded engine team. Do not market a general-purpose browser or open
an unreviewed build to arbitrary browsing to create the appearance of progress.
