# JavaScript VM feasibility: first investigation

Date: 16 November 2026. Work item: B030. Status: **started; no VM selected and no
embedding gate passed**. The current BroWSER executable does not embed a JavaScript
runtime. Script nodes are inert fixture data. This investigation can run alongside
M1; enabling scripts still depends on the M2 containment gate.

## Sources inspected

The [SpiderMonkey project](https://spidermonkey.dev/) links its C++ embedding examples
and Servo's Rust bindings. The Rust route is a maintained candidate to evaluate,
not evidence that it already works with BroWSER. The following immutable revisions
were resolved and inspected through GitHub on the date above:

| Source | Revision and observed facts |
| --- | --- |
| [Servo Rust bindings](https://github.com/servo/mozjs/tree/37f1c8fc8ebe7ad26cd88fdb9b203a28fe1860da) | `37f1c8fc8ebe7ad26cd88fdb9b203a28fe1860da`; `mozjs` version `0.27.1`, with exact `mozjs_sys` dependency `153.7.0-1` |
| [Bundled engine source marker](https://github.com/servo/mozjs/blob/37f1c8fc8ebe7ad26cd88fdb9b203a28fe1860da/mozjs-sys/etc/COMMIT) | Records upstream source revision `a9effd9e521653405f0ffd311e8edf90bde92237`; this marker must accompany the binding revision in future build evidence |
| [Binding toolchain](https://github.com/servo/mozjs/blob/37f1c8fc8ebe7ad26cd88fdb9b203a28fe1860da/rust-toolchain.toml) | Pins Rust `1.88.0`; BroWSER currently uses `1.100.0`. Compatibility with the newer compiler has not been tested |
| [Mozilla embedding examples](https://github.com/mozilla-spidermonkey/spidermonkey-embedding-examples/tree/8474c9fefba9d89e1ab74efbe238fe7877d473ad) | `8474c9fefba9d89e1ab74efbe238fe7877d473ad` on `esr115`; useful API/ownership examples, but not a version-matched implementation recipe for the binding snapshot above |

The binding repository exposes separate low-level and higher-level crates. Its
[source build path](https://github.com/servo/mozjs/blob/37f1c8fc8ebe7ad26cd88fdb9b203a28fe1860da/mozjs-sys/makefile.cargo)
uses the Mozilla configure/Make machinery and Clang bindings. Mozilla also documents
a [standalone shell build and test path](https://firefox-source-docs.mozilla.org/js/build.html).
Neither route has been built during this investigation. A successful standalone
shell would be a prerequisite baseline, not proof of browser embedding correctness.

## License identification

The binding workspace declares `MPL-2.0` in its
[Cargo manifest](https://github.com/servo/mozjs/blob/37f1c8fc8ebe7ad26cd88fdb9b203a28fe1860da/Cargo.toml),
and [mozjs/LICENSE](https://github.com/servo/mozjs/blob/37f1c8fc8ebe7ad26cd88fdb9b203a28fe1860da/mozjs/LICENSE)
contains the Mozilla Public License 2.0 text. The bundled engine's
[LICENSE file](https://github.com/servo/mozjs/blob/37f1c8fc8ebe7ad26cd88fdb9b203a28fe1860da/mozjs-sys/mozjs/LICENSE)
directs readers to its full third-party license inventory. Mozilla's example
repository has its own
[Apache-2.0-or-MIT license notice](https://github.com/mozilla-spidermonkey/spidermonkey-embedding-examples/blob/8474c9fefba9d89e1ab74efbe238fe7877d473ad/LICENSE).

These are source/license identifications, not a completed distribution review.
Before adoption, inventory the selected engine, binding, generated wrappers,
transitive libraries, patches and any binary archives. Preserve their applicable
notices and satisfy their obligations; BroWSER's MIT license does not replace them.

## Local environment observed

Read-only probes ran on the current macOS arm64 development host. They establish
availability, not compatibility with the engine's build system.

| Probe | Observed result |
| --- | --- |
| BroWSER Rust compiler | `rustc 1.100.0 (0ef1bc386 2026-11-09)`, installed under `artifacts/toolchains` |
| C/C++ compiler | Apple Clang `21.0.0`; `clang` and `clang++` available |
| Shared Clang library | Xcode `usr/lib/libclang.dylib` exists at the probed toolchain path; binding discovery has not been run |
| Python | `3.15.2`; compatibility with this Mozilla source snapshot is untested |
| Make | GNU Make `3.81` available as `make`; `gmake` unavailable on PATH |
| Other tool inventory | `m4` and Perl available; `cmake`, `ninja`, `autoconf`, and `pkg-config` unavailable on PATH |
| Archive attestation tooling | `gh` unavailable on PATH; upstream's `gh`-based prebuilt attestation route cannot run in this environment yet |
| Node.js | `v22.23.1` available; this is not a BroWSER VM embedding or browser conformance result |
| Linux runtime | No Linux VM build, sandbox run, or engine test was executed in this investigation |

The missing tools above are an inventory, not a claim that all are mandatory for
the selected source-build route. Establish actual prerequisites from the pinned
build and record its diagnostics before choosing additional installations.

Reproduce the local observations from the repository root:

```sh
uname -sm
python3 --version
node --version
clang --version
make --version
for tool in clang++ m4 perl cmake ninja autoconf pkg-config gh gmake; do
    command -v "$tool" || true
done
test -f /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/libclang.dylib
CARGO_HOME="$PWD/artifacts/toolchains/cargo" \
RUSTUP_HOME="$PWD/artifacts/toolchains/rustup" \
artifacts/toolchains/cargo/bin/rustc --version
```

No VM source archive was downloaded, no VM library was linked, no GC wrapper was
compiled, and no VM footprint or conformance measurement was produced. The binding
metadata and small source/license files were inspected remotely.

## Experiments required to close B030

The following are experiment contracts, not callable placeholder APIs:

1. **Reproducible builds:** build the exact binding/engine revision on Linux x86_64
   and the macOS developer host. Record compiler, Python, SDK, flags, dependencies,
   elapsed build time, peak build memory, binary size, source provenance and logs.
   Exercise both a debug/GC-stress configuration and the intended runtime settings.
2. **GC-safe document wrapper:** expose one real native document and element through
   an audited FFI boundary. Preserve wrapper identity, trace native-to-JS references,
   handle detached nodes and cycles, and tear down a document while references remain.
   Force collections between operations and fail on a stale or wrongly owned wrapper.
   The [Mozilla GC rooting guide](https://github.com/mozilla-spidermonkey/spidermonkey-embedding-examples/blob/8474c9fefba9d89e1ab74efbe238fe7877d473ad/docs/GC%20Rooting%20Guide.md)
   explains why moving-GC references must be rooted/traced. Its examples need review
   against the chosen engine version rather than copying unchanged code.
3. **Host semantics:** demonstrate separate realms, exceptions, interruptible runaway
   execution, promise jobs, module loading and navigation cancellation. Verify that
   one context cannot obtain another context's native authority. These tests are
   distinct from evaluating a standalone JavaScript expression.
4. **Baseline and update rehearsal:** run the standalone and embedded VM with the
   same pinned Test262 cohort and report execution variants, failures and exclusions.
   Rehearse updating the engine/bindings, rerun ownership and security cases, and
   measure the integration maintenance needed to keep releases patched.
5. **Selection review:** compare actual results against the independent-engine ADR,
   including JIT permissions, sandbox impact, footprint and update ownership. If the
   Rust route fails its requirements, compare another maintained embedding route
   before selecting a VM. An unbuilt candidate cannot pass this review.

These experiments supply B030 evidence only. Web IDL/DOM bindings, the HTML event
loop, browser-origin enforcement and the M3 dynamic-platform gate remain separate
implementation obligations.
