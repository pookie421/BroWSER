# ADR 0001: independent rendering core with maintained supporting components

Date: 16 November 2026. Status: accepted for planning. Dependency selection remains
conditional on integration evidence; no dependencies are installed by this decision.

## Context

An independent engine must own how HTML becomes a live document, how CSS resolves,
how layout is computed, and how changes reach pixels. Reimplementing cryptography,
font shaping, compression and a production JavaScript optimizing compiler at the
same time would expand the critical path without proving that rendering architecture.

## Decision

First-party engine implementation will use Rust. Custom subsystems will include HTML
tokenization and tree construction, DOM ownership and mutation, selectors and cascade,
layout and invalidation, display lists, browser integration, and policy enforcement.
Avoid an unsafe-free marketing claim: operating-system interfaces, a JS VM, shaping,
decoders and GPU drivers still create native boundaries requiring review.

| Component | Direction | Evidence required before adoption |
| --- | --- | --- |
| JavaScript/WebAssembly | Evaluate SpiderMonkey first; compare V8 if embedding viability fails | Linux and macOS reproducible builds, license inventory, DOM tracing prototype, interrupted execution, jobs/modules, memory measurement and a maintained upstream update procedure |
| Text and fonts | Evaluate HarfBuzz and maintained font/raster libraries | Mixed-direction and complex-script shaping, deterministic test fonts, sandbox resource loading and malformed-font fuzzing |
| TLS and HTTP | Integrate maintained libraries; implement browser Fetch policy separately | Certificate hostname/trust validation, redirect and credential tests, cancellation, streaming limits and dependency update ownership |
| URL/encoding | Adopt a conforming maintained library only if upstream test results support it | Pinned WHATWG fixture results, IDNA and opaque-origin cases, cross-service serialization consistency |
| Image/media | Narrow reviewed codec set first, separate decoding service | Decompression limits, malformed-stream fuzzing, sandbox tests and license/notice inventory |
| Raster/GPU | CPU reference path first; compare maintained raster/GPU libraries | Stable display-list contract, pixel tests, crash recovery and measured frame/memory behavior |

No VM is selected merely because it executes `1 + 1`. The feasibility result must
demonstrate GC-safe native wrappers, realm separation, error propagation, promise jobs,
build/update maintenance and the impact of JIT on sandbox policy. A standalone VM
does not supply DOM, Web APIs, the HTML event loop or the browser security model.

All adopted source and tools must be pinned by immutable revision or package lock,
with provenance, license/notice obligations, patch policy, a named role responsible
for updates, and reproducer tests for local patches. Component licenses do not become
MIT merely because this repository uses MIT. Review obligations before distribution.

## Alternatives and consequences

A Chromium fork or web-view shell would shorten time to broad compatibility but
would not meet the independent rendering-core requirement. A fully novel JS VM is
reserved for a separately funded research proposal; it is not on the initial critical
path. Adopting an existing complete independent engine changes the product premise
and requires an explicit new architecture decision.

Rust reduces some memory-management hazards in first-party code; it does not remove
logic flaws, denial of service, unsafe FFI risk or a need for isolation. Reusing a VM
also ties releases to upstream changes. Budget for integration maintenance and
benchmark the total product rather than attributing VM performance to custom code.

Review this ADR at the M1 investment decision and whenever integration prototypes
invalidate an assumption. Record measured outcomes and the replacement decision;
do not retain an unworkable integration behind an empty abstraction.

## Primary references

Accessed 16 November 2026:

- [SpiderMonkey embedding documentation](https://spidermonkey.dev/).
- [V8 embedding guide](https://v8.dev/docs/embed).
- [HarfBuzz manual](https://harfbuzz.github.io/).
- [WHATWG URL Standard](https://url.spec.whatwg.org/).
- [WHATWG Fetch Standard](https://fetch.spec.whatwg.org/).
