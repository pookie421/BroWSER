# Planning baseline verification

Date: 16 November 2026. Scope: M0 and B001 only. Project location:
[pookie421/BroWSER](https://github.com/pookie421/BroWSER), public, default branch `main`.

## Environment and commands

Local verification uses macOS, Python 3.15.2, and the Python standard library.
The tools target Python 3.11 or later; the configured Ubuntu 24.04 CI job supplies
an additional supported environment. CI results belong to the exact GitHub commit
and are available in the repository's Actions page; this record does not invent an
unobserved CI result.

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
| GitHub Actions | Configured | Read-only workflow with a pinned official checkout action |
| Engine, browser executable and Web APIs | Unimplemented future work | M1–M5 and all corresponding task statuses remain planned |
| Engine standards/performance/security results | Not measured | No scores, engine binaries or passing engine gates are asserted |

The baseline underwent agent-assisted architecture and consistency review. This is
planning quality assurance, not independent human security certification or evidence
that proposed isolation mechanisms work. Engine completion requires real artifacts
and the reviews defined for its milestone. The validator checks structure and file
references; it cannot judge whether prose or a test result proves a requirement.

This file is an evidence index for the published planning-baseline commit. The exact
commit identity is available from its Git history, without a self-referential hash
inside the file. Future changes require rerunning the relevant checks.
