# Coverage-guided engine fuzzing

The standalone fuzz workspace uses cargo-fuzz 0.13.2, libfuzzer-sys 0.4.13
and Rust nightly from 16 November 2026. The lockfile pins the runtime and all
Rust dependencies. The engine test and build commands use the root workspace.
[cargo-fuzz release](https://github.com/rust-fuzz/cargo-fuzz/releases/tag/0.13.2) ·
[libfuzzer-sys changelog](https://github.com/rust-fuzz/libfuzzer/blob/main/CHANGELOG.md)

- *html_parse* exercises the strict parser, document parser and div fragment
  parser. It checks deterministic parse results, reciprocal parent/child links,
  node and attribute limits, and reachability from the document root.
- *render_fixture* exercises the bootstrap and M1 profiles at viewports up
  to 64 pixels per dimension. It checks the PPM dimensions and compares
  a second render in the same process, including diagnostics. The first three
  bytes select profile, width and height. The remaining bytes are UTF-8 HTML.

From the repository root, provision the fuzz tool and packages once.

~~~sh
rustup toolchain install nightly-2026-11-16 --profile minimal
cargo install --locked --version 0.13.2 cargo-fuzz
cargo +nightly-2026-11-16 fetch --locked --manifest-path fuzz/Cargo.toml
~~~

Run the smoke campaign from the repository root on macOS or Linux.

~~~sh
python3 tools/run_m1_fuzz.py --runs 5000
~~~

The driver builds both address-sanitized targets offline from a verified
source snapshot, seeds them from the frozen synthetic HTML assets, and
executes them concurrently in the foreground. It checks source identity
throughout. Each fresh `artifacts/m1/fuzz-*` directory holds the build log,
fuzzer logs, retained corpus and any failures. The report identifies the
toolchain, source and binary hashes, configuration, seeds, coverage, CPU
time and process peak RSS. An error or interruption leaves an incomplete
report and ends both child processes. [Campaign driver](../tools/run_m1_fuzz.py)

For a sustained observation on the selected worker, use a timed budget of
seven days plus a few seconds for process startup and polling.

~~~sh
python3 tools/run_m1_fuzz.py --seconds 604805
~~~

The timed report records the overlap observed between the two target processes.
A seven-day observation requires both targets to complete and at least
604,800 seconds of overlap. Each target uses libFuzzer's sampled RSS check
and a three-second per-input timeout. The driver also checks each process's
peak RSS at exit and bounds retained evidence. The milestone still requires
independent replay and review. [libFuzzer flags](https://www.llvm.org/docs/LibFuzzer.html) ·
[Evidence requirements](../docs/VALIDATION.md#determinism-bounds-and-adversarial-runs)
