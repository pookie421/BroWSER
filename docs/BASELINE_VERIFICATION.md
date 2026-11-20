# Planning baseline verification

Date: 16 November 2026. Scope: M0 and B001 only. Project location:
[pookie421/BroWSER](https://github.com/pookie421/BroWSER), public, default branch `main`.

This is a historical record of M0's published planning baseline. Implementation
has since started; see the [current M1 matrix](../evidence/M1/capability-matrix.md).
The planned statuses and 28-test count below describe the linked baseline commit.

**M0: complete. B001: complete.** Rechecked against the published baseline commit
[`fe85687`](https://github.com/pookie421/BroWSER/commit/fe856876d7490f78083ecf01c1904b43a15e97a8).
All ten M0 deliverables listed in the machine-readable plan exist in that commit.

## Acceptance checklist

| M0 requirement | Result | Evidence |
| --- | --- | --- |
| Strategy, architecture, release gates and staffing assumptions agree | Passed planning consistency review | [Strategy](STRATEGY.md), [architecture](ARCHITECTURE.md), [security model](SECURITY_MODEL.md), [roadmap](ROADMAP.md), and [resourcing](RESOURCING.md); M2/M3 are internal builds and external network distribution requires B042 operations gates |
| Repository validation and negative contract tests pass | Passed | Fresh local `make check`: 28 tests passed and plan validation passed; published CI completed successfully |
| Public repository contains the reviewed baseline with honest implementation status | Passed | Public `main` contains the ten M0 deliverables; [README](../README.md) identifies the planning-only status; M1–M5 remain planned in [the plan](../planning/roadmap.json) |

M0 closes the planning baseline only. The next ready work item is B010, which freezes
the M1 renderer scope, test IDs, limits, fonts and upstream revisions. M1 implementation
and its completion evidence are separate future work.

## Environment and commands

Local verification uses macOS, Python 3.15.2, and the Python standard library.
The tools target Python 3.11 or later. The Ubuntu 24.04 CI run for baseline commit
`fe856876d7490f78083ecf01c1904b43a15e97a8` completed successfully:
[Planning integrity, run 1](https://github.com/pookie421/BroWSER/actions/runs/43855227492).
Its `validate` job and `Verify planning contracts` step both report success. This is
repository verification, not engine conformance evidence.

```sh
python3 -m unittest discover -s tests -v
python3 tools/check_plan.py
```

Observed `make check` result: **28 tests passed; repository validation passed**.
Tests exercise valid/invalid plans,
exact schema, duplicate IDs/JSON keys, cycles, unknown dependencies, completion gates,
milestone ordering, missing evidence/deliverables, symlink/path escapes, local links,
and CLI exit status. Repository validation passed the schema, dependency graph,
evidence-path and local Markdown file-link checks.

## Capability and review boundary

| Capability | State | Evidence |
| --- | --- | --- |
| Strategy, architecture and threat model | Documented planning decisions | Strategy, architecture, security and dependency ADR documents |
| Delivery and staffing plan | Explicit assumptions and gated contracts | Roadmap, resourcing and 27 work items in the JSON plan |
| Plan validator | Implemented and tested | Python tool and negative contract tests |
| Local documentation file links | Checked by validator | Existing local targets; external availability and fragment anchors are outside this check |
| GitHub Actions | Baseline run passed | Read-only workflow with a pinned official checkout action; linked run 1 |
| Engine, browser executable and Web APIs | Unimplemented future work | M1–M5 and all corresponding task statuses remain planned |
| Engine standards/performance/security results | Not measured | No scores, engine binaries or passing engine gates are asserted |

The baseline underwent agent-assisted architecture and consistency review. This is
planning quality assurance, not independent human security certification or evidence
that proposed isolation mechanisms work. Engine completion requires real artifacts
and the reviews defined for its milestone. The validator checks structure and file
references; it cannot judge whether prose or a test result proves a requirement.

This file is an evidence index for the explicitly linked planning-baseline commit.
Its own acceptance-record update has a separate identity in Git history. Future
changes require rerunning the relevant checks; baseline CI evidence does not claim
to verify later commits.
