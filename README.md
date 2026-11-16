# BroWSER

An independent browser engine project with an auditable path from a deterministic
offline renderer to a secure, interoperable desktop browser.

**Status: planning baseline, 16 November 2026.** This repository contains the
engineering plan and working plan-validation tools. It contains no browser engine
implementation and makes no compatibility, performance, or security claims for an
engine. Every engine milestone remains planned.

**M0 is complete.** The [acceptance record](docs/BASELINE_VERIFICATION.md) maps
each baseline requirement to its evidence. The next work item is B010: freeze the
M1 offline renderer's capability and fixture contract before implementing the engine.

The ambition is to compete with Chromium/Blink and Firefox/Gecko on compatibility,
security, responsiveness, resource use, and developer trust. Achieving that requires
sustained, specialist engineering over multiple years. The first delivery target is
much narrower: a reproducible, offline HTML/CSS renderer on Linux.

## Start here

| Document | Purpose |
| --- | --- |
| [Product strategy](docs/STRATEGY.md) | Users, differentiation, scope, and success criteria |
| [Architecture](docs/ARCHITECTURE.md) | Rendering pipeline, process boundaries, and integration decisions |
| [Roadmap](docs/ROADMAP.md) | Milestone gates and the first 90 days |
| [Security model](docs/SECURITY_MODEL.md) | Threats, controls, and browsing release gates |
| [Validation](docs/VALIDATION.md) | Standards, compatibility, fuzzing, and benchmark protocols |
| [Resourcing](docs/RESOURCING.md) | Staffing assumptions, scheduling, and operating costs |
| [Dependency decisions](docs/decisions/0001-engine-boundary.md) | What to build and what to integrate |
| [Work items](docs/BACKLOG.md) | Prioritized implementation contracts |
| [Machine-readable plan](planning/roadmap.json) | Dependencies, acceptance criteria, and evidence state |
| [M0 acceptance record](docs/BASELINE_VERIFICATION.md) | Baseline checklist, local checks, and published CI evidence |
| [Standards sources](docs/SOURCES.md) | Authoritative references and evidence policy |

## Validate the repository

Install Python 3.11 or later. No third-party packages are required.

```sh
python3 tools/check_plan.py
python3 -m unittest discover -s tests -v
```

Or run `make check`. These checks validate the planning package, dependency graph,
completion evidence, and local documentation links. They are not engine tests.
The same checks run in [GitHub Actions](.github/workflows/plan.yml).

## Engineering commitments

- Build a custom DOM, style system, layout pipeline, and display-list architecture.
- Use Rust for first-party engine code and explicit reviewed boundaries for native dependencies.
- Evaluate an established JavaScript VM before committing to an embedding strategy.
- Establish process isolation and origin enforcement before enabling public network browsing.
- Pair each implemented capability with a specification reference and meaningful tests.
- Report unsupported features and unexecuted tests honestly. Publish denominators with scores.
- Merge complete vertical slices. Empty handlers, success-shaped fallbacks, disabled checks,
  and unimplemented functions cannot satisfy a milestone.

The plan describes future work without generating empty crates or pretending that
the described subsystems already run. [Contributing](CONTRIBUTING.md) defines the
acceptance process; [SECURITY.md](SECURITY.md) defines vulnerability reporting.

## License

[MIT](LICENSE). Third-party components will retain their own licenses and notices;
none are bundled in this planning baseline.
