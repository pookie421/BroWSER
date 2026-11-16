# Team, capacity, and operating plan

Planning assumptions dated 16 November 2026. These are engineering scenarios, not quotes, staffing commitments, or a claim that funding exists. Actual costs depend on location, compensation, procurement, platform support, and contractual obligations. Milestone acceptance is controlled by evidence in [ROADMAP.md](ROADMAP.md), not expenditure.

## Staffing scenarios

| Scenario | Capacity assumption | Credible purpose | Constraint |
| --- | --- | --- | --- |
| Individual research | 1–2 systems engineers; part-time independent review | Offline renderer experiments and tightly scoped tools; explore architecture | Cannot responsibly promise a secure general-purpose browser or continuous response coverage |
| Focused engine team | 8 engineers plus funded specialist review and 1 product/operations role | First 90-day program; M1 candidate; security and VM feasibility in parallel | Hiring, onboarding, and unfamiliar specifications can exceed the first quarter |
| Sustained browser program | 25–40 engineers plus dedicated security, QA/release, accessibility/design, and operations capacity | Concurrent interoperability work, supported releases, platform ports, and independent audit remediation | A planning scenario for multi-year execution; headcount does not establish competitive readiness |

The eight-engineer planning team allocates one technical lead; one HTML/DOM engineer; one CSS/cascade engineer; two layout/text/rendering engineers; one platform/network/sandbox engineer; one JavaScript/bindings engineer; and one test/performance/release engineer. Security review, accessibility review, and independent code review need separately reserved capacity. A single engineer can contribute across areas, but no milestone's sole author approves its own evidence.

Before M2, add dedicated security engineering and a second platform engineer so broker policy and network implementation receive independent review. Before M4, staff a release owner, accessibility owner, site-compatibility triage, and documented security response rotation. Before M5, every critical subsystem needs two people able to review and release it; vacations and departure must not disable security updates.

## Capacity model

Use this spreadsheet-independent model in a quarterly budget review:

`quarterly_cost = 3 × (engineers × monthly_loaded_engineer_cost + other_staff_monthly_cost + infrastructure_monthly_cost) + quarterly_audit_cost + quarterly_device_cost + quarterly_other_cost`

`planned_cash_need = quarterly_cost × quarters + contingency + orderly_shutdown_reserve`

`usable_engineering_weeks = engineers × calendar_weeks × availability_fraction`

Loaded cost includes salary/contract, employer costs, equipment, and benefits as applicable. Count specialist contractors in either staff or audit costs, without double counting. Availability includes leave, onboarding, review, incidents, and maintenance; use 0.65 as an initial planning assumption, then replace it with observed team throughput. For eight engineers over thirteen weeks, that assumption yields 67.6 engineering weeks, not 104 weeks of feature work.

Illustrative inputs, deliberately not market estimates: eight engineers at 10,000 currency units per loaded month; other staff at 10,000 per month; infrastructure at 5,000 per month; audits at 25,000 per quarter; devices at 10,000 per quarter; other costs at 5,000 per quarter. This produces 325,000 per quarter before contingency. A 25% contingency yields 406,250 for that quarter, excluding any separate shutdown reserve. Replace every input with a local approved budget before committing work.

Do not publish a competitive release date based only on this cost model. Forecast with demonstrated throughput, defect trend, external dependency health, and the critical path. Re-estimate quarterly and when a VM, rendering library, or supported platform changes.

## Infrastructure and maintenance

Fund Linux workers for fast change checks, sanitizer/debug builds, continuous fuzz targets, reference browser runs, and clean release builds. Separate untrusted corpus execution from signing credentials. Add GPU machines and supported OS hardware when their milestones require them. Reserve artifact storage for raw compatibility/performance/security evidence, minimized failures, symbols, and signed packages. Apply documented retention and secret-redaction rules.

Reserve at least 30% of ongoing engineering capacity for defects, dependency updates, test maintenance, documentation, release work, and security response after M2. Revisit the percentage from actual incident and backlog data. Feature commitments consume the remainder. Stop new feature intake when a supported release has an unowned security patch or the release branch cannot reproduce its evidence.

Maintain a dependency inventory with upstream source, pinned revision, license, security contact, update cadence, and internal owner. Assign an owner for upstream compatibility changes, especially VM embedding and graphics/font libraries. Budget time to upstream fixes and test cases; maintaining a private fork is a recurring cost, not a one-time integration.

## Operating responsibilities

| Responsibility | Accountable role | Required operating artifact |
| --- | --- | --- |
| Architecture and scope | Technical lead | Reviewed decisions, capability matrix, milestone scope changes |
| Standards interoperability | Subsystem owners and test lead | Per-feature test mapping, full-suite outcome counts, upstream issues |
| Security and privacy | Security lead | Threat-model review, vulnerability triage, privileged-path approval |
| Releases and updates | Release owner | Reproducible build evidence, signatures, rollback and recovery drills |
| User experience and accessibility | Product/accessibility owner | Task corpus, keyboard/screen-reader evidence, known limitations |
| Funding and continuity | Project owner | Quarterly capacity plan, runway review, maintenance and shutdown reserves |

Before distributing a public-network build, establish a private vulnerability reporting channel and a staffed response process. Target acknowledgement within two business days. An active exploit or critical remote issue triggers immediate triage, a release block, and a mitigation decision within 24 hours; those are service objectives that require actual staffing. Establish a clear supported-version policy and emergency update mechanism before promising users a support window.

An external security review needs a written scope, reviewer independence, evidence access, a remediation budget, and a retest. A commissioned review does not pass a gate until blocking findings are fixed or the affected feature is removed from the supported product.

## Funding checkpoints and continuation

At the end of each quarter, compare verified milestones, defect backlog, actual cost, reviewer capacity, and remaining funded runway. Proceed to M2 only with resources for its security and maintenance obligations. Proceed to a public alpha only with a funded update and vulnerability-response plan. The project owner records the decision, revised scope, and budget assumptions publicly where confidentiality permits.

If funding or key personnel become insufficient, stop expanding supported capabilities, keep the last supported branch patched within the announced window, publish status, and provide a documented export/migration path for profiles. Preserve licenses, source, test evidence, and reproducible builds. Ending a research milestone cleanly is preferable to distributing software whose update obligations cannot be met.

The goal of these gates is continuity: a browser project needs reliable security releases and sustained compatibility work after the first successful demo. No part of this plan substitutes a nominal headcount or budget for those operating capabilities.
