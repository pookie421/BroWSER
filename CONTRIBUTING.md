# Contributing

BroWSER is implementing M1's offline renderer. Start with [README.md](README.md),
[the frozen M1 scope](docs/M1_SCOPE.md) and [the backlog](docs/BACKLOG.md). Small
complete changes with reproducible evidence are the unit of contribution. The
bootstrap is a developer tool; no general-purpose browser release exists yet.

## Working process

Select a ready work item whose dependencies have evidence. Describe the precise
behavior, relevant specification sections, supported scope, and expected error cases.
If a decision changes process boundaries, privilege, ownership, dependencies or public
behavior, record an architecture decision before expanding implementation.

Each implementation PR must include a usable path through the changed subsystem,
tests that can fail on the relevant defect, and a command that reproduces the result.
Do not merge empty handlers, invented fixture results, unconditional success paths,
commented-out validation, `todo!()`, `unimplemented!()`, or a `NotImplementedError`
where a promised behavior should run. Future scope belongs in the backlog; it does
not belong in a callable API that pretends to support it. Unsupported web features
must follow the applicable specification's failure or fallback behavior.

## Checks

Use Python 3.11 or later:

```sh
python3 tools/check_plan.py
python3 -m unittest discover -s tests -v
make engine-check
```

Changes to `planning/roadmap.json` must preserve acyclic dependencies and honest
completion evidence. Complete and in-progress entries cannot depend on unfinished
entries. M0 evidence concerns the planning package only. Engine changes also need
Rust tests/Clippy and the relevant frozen fixture assertions. Use the bootstrap
denominator honestly; unimplemented full-M1 cases remain not run.

## Review and evidence

Include commit and tool versions, exact commands, input revisions, output artifacts,
and a concise description of failures or exclusions. Do not commit secrets, real
browsing profiles, private site captures, or generated build directories. Use synthetic
fixtures and legally redistributable test assets with their licenses.

The repository maintainer reviews scope and integration. Security-sensitive process,
IPC, origin, VM/GC or dependency changes also need review by a person responsible for
that domain before a browser release. Role names in the plan are staffing requirements,
not a claim that those people are currently hired. Initially the maintainer is
[@pookie421](https://github.com/pookie421).

Contribution is under the [MIT license](LICENSE). Do not add third-party code without
its original license, notices, provenance and update responsibilities. Report suspected
vulnerabilities using [SECURITY.md](SECURITY.md).
