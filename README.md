# BroWSER

An independent browser engine project with an auditable path from a deterministic
offline renderer to a secure, interoperable desktop browser.

**Status: M1 in progress, 21 November 2026.** The offline Rust engine inspects
and mutates controlled DOM trees, computes CSS values, and paints block boxes,
declared PNGs, and pinned-font text to PPM. The M1 text profile covers standalone
blocks with homogeneous Latin, Arabic, or Devanagari text. Scripting, networking,
and the desktop browser remain future work.

**M0 is complete.** The [acceptance record](docs/BASELINE_VERIFICATION.md) maps
each baseline requirement to its evidence. B010's M1 scope and fixtures are frozen.
The [current capability matrix](evidence/M1/capability-matrix.md) separates working
bootstrap behavior from the remaining M1 gates.

**M2 preparation** includes a draft acceptance cohort and an offline broker
prototype using synthetic child processes. The renderer stays offline while M1
acceptance and Linux containment are pending. [M2 scope](docs/M2_SCOPE.md)

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
| [M1 scope](docs/M1_SCOPE.md) | Frozen capabilities, test oracles, fonts, resources and limits |
| [M2 acceptance candidate](docs/M2_SCOPE.md) | Broker prototype, proposed isolation and network gates, and work sequence |
| [Bootstrap evidence](evidence/M1/BOOTSTRAP.md) | Actual checks, repeatability and remaining gates |
| [Code audit](docs/CODE_AUDIT.md) | Reproduced findings, remediation and outstanding milestone work |
| [VM feasibility](docs/VM_FEASIBILITY.md) | Parallel source and build-prerequisite investigation |
| [M0 acceptance record](docs/BASELINE_VERIFICATION.md) | Baseline checklist, local checks, and published CI evidence |
| [Standards sources](docs/SOURCES.md) | Authoritative references and evidence policy |

## Validate the repository

Install Python 3.11 or later. No third-party packages are required.

```sh
python3 tools/check_plan.py
python3 -m unittest discover -s tests -v
```

Or run `make check`. These checks validate the planning package, dependency graph,
completion evidence, and repository Markdown file targets.
The same checks run in [GitHub Actions](.github/workflows/plan.yml).

## Build and run the renderer

Install Rust using its official installer. `rust-toolchain.toml` selects Rust 1.100.0.
Fetch the checksum-locked Cargo dependencies once. Builds and fixture replays
then run offline.

```sh
cargo fetch --locked
cargo build --locked --offline
mkdir -p artifacts/demo
cargo run --locked --offline -- inspect --fixture fixtures/m1/bootstrap/root-box.html --output artifacts/demo/dom.json
cargo run --locked --offline -- render --fixture fixtures/m1/bootstrap/nested-boxes.html --output artifacts/demo/image.ppm --layout artifacts/demo/layout.json --width 64 --height 48
cargo run --locked --offline -- render --profile m1 --fixture fixtures/m1/full/arabic.html --output artifacts/demo/arabic.ppm --layout artifacts/demo/arabic.json --width 128 --height 64
cargo run --locked --offline -- render --fixture fixtures/m1/full/png.html --resource checker.png=fixtures/m1/assets/checker.png --output artifacts/demo/checker.ppm --width 8 --height 8
make engine-check
```

For document and fragment inspection, pass `--parse-mode document` or
`--fragment-context div`. The default still accepts the strict bootstrap grammar.
Use fresh output filenames for each invocation.
The `style` command reports computed values, `shape` reports glyph positions and
UTF-8 clusters, and `decode-png` checks dimensions and PNG integrity. Fixture and
resource files must resolve inside the selected fixture root (`fixtures/m1` by
default). Use controlled synthetic inputs for this unsandboxed developer build.
[Supported grammar and CLI](docs/M1_SCOPE.md#current-engine-profiles)

`make engine-check` runs Rust formatting, tests, Clippy, and the bootstrap replay
from a fresh Cargo build. The replay verifies the reviewed cohort and its assets,
checks independent pixels and geometry, exercises each rejected feature alongside
its passing control, and repeats each render case 100 times. Each invocation
publishes a new bundle under `artifacts/m1/` with the build record, diagnostics
and available failure artifacts. Run the 23-case development cohort separately
with `python3 tools/run_m1_bootstrap.py --stage full-m1 --repeat 100`.

## Engineering commitments

- Build a custom DOM, style system, layout pipeline, and display-list architecture.
- Use Rust for first-party engine code and explicit reviewed boundaries for native dependencies.
- Evaluate an established JavaScript VM before committing to an embedding strategy.
- Establish process isolation and origin enforcement before enabling public network browsing.
- Pair each implemented capability with a specification reference and meaningful tests.
- Report unsupported features and unexecuted tests honestly. Publish denominators with scores.
- Merge complete vertical slices. Empty handlers, success-shaped fallbacks, disabled checks,
  and unimplemented functions cannot satisfy a milestone.

[Contributing](CONTRIBUTING.md) defines the acceptance process;
[SECURITY.md](SECURITY.md) defines vulnerability reporting. Proposed architecture
is introduced through complete tested slices as capabilities become real.

## License

[MIT](LICENSE) for first-party code and original fixtures. The bundled pinned Noto
fonts retain their [SIL Open Font License notices](fixtures/m1/assets/README.md).
