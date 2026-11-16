# BroWSER security model

Status: design and release policy, 2026-11-16. The engine, isolation mechanisms,
and security tests described here are planned. No browsing safety or security
certification is claimed by this planning repository.

## Assets and adversaries

Protect the user's local files, profile databases, credentials, authenticated web
sessions, browsing history, device capabilities, trusted UI, and the integrity of
installed updates. Protect one web origin's data from another origin and isolate
private sessions from normal profiles and telemetry.

Assume an attacker controls HTML, CSS, script, fonts, images, response headers,
redirects, downloads, cross-origin frames, and timing. Assume a content process
can become fully compromised. Include a compromised codec worker, malicious IPC
peer, network observer, hostile site attempting resource exhaustion, and tampered
update mirror. Rust reduces a category of memory errors but does not authorize
data access, prevent logic errors, or make native libraries safe.

An already compromised host kernel or administrator account is outside the
browser's containment guarantee. Record that boundary without treating it as an
excuse for avoidable process privilege or unsafe defaults.

## Required invariants

1. No content process opens arbitrary filesystem paths, creates raw network
   connections, reads profile databases, or invokes general desktop services.
2. The broker creates and owns security context: committed origin, top-level
   partition, navigation generation, process assignment, user activation, and
   granted capabilities. A child cannot redefine these in a request payload.
3. A sandbox is installed and verified before any untrusted page bytes reach a
   process. If setup fails, remote content does not load. A development bypass
   cannot be enabled accidentally in a release package.
4. No IPC message, shared-memory range, renderer frame, accessibility update, or
   permission result is accepted without bounds, identity, and lifetime checks.
5. Cross-origin access requires the applicable web-platform permission and
   isolation checks. A process boundary supplements those checks; it does not
   replace same-origin enforcement.
6. Certificate verification failures block the connection. Privileged internal
   pages and origin indicators are drawn by trusted UI, outside page-controlled
   surfaces.
7. Permissions are narrow, revocable, tied to the correct context, and denied
   when required activation or visibility is absent. Feature detection never
   reports an unsupported protected API as available.
8. Release packages verify update authenticity and reject unauthorized rollback;
   emergency recovery has a separate authenticated procedure.

## Threat register and evidence

| Threat | Required control | Minimum adversarial evidence before exposure |
| --- | --- | --- |
| Memory corruption in parser, VM, codec, or FFI | Safe Rust by default; reviewed unsafe boundary; untrusted work in restricted processes; prompt dependency updates | Fuzz corpus, sanitizer/native diagnostics, GC stress, crash triage, audited FFI ownership |
| Renderer compromise and sandbox escape | OS-enforced role-specific restrictions and narrow broker capabilities | Attempt denied file reads/writes, socket creation, process creation, executable memory where forbidden, and broker message abuse from a deliberately hostile child |
| Confused deputy through IPC | Broker-derived authority, generation-bound handles, message limits, capability revocation | Cross-origin, stale-generation, forged-handle, duplicate-request, oversized-message, and disconnect tests |
| Origin confusion or navigation races | Shared URL/origin types, final-response validation, isolated context assignment | Opaque-origin, redirects, `about:blank`, inherited-origin, popup, history, and cross-origin-frame cases |
| Credential or partition leakage | Network-owned cookies, storage keys, response tainting, cache partitioning | Two controlled sites and private/normal profiles cannot read or reuse forbidden cookies, cache entries, authentication, or storage |
| UI spoofing and dangerous permissions | Broker-owned origin display, download handling, permission UI, activation checks | Fullscreen, focus stealing, popup, forged-origin text, Unicode-host display, stale permission, and click routing tests |
| Malicious media or GPU commands | Sandboxed codecs, decoded-size limits, validated resources, separate GPU role | Truncated/recursive/oversized input corpus, shared-buffer mutation, bad resource IDs, driver loss and process crash recovery |
| CPU, memory, storage, or IPC exhaustion | Per-job limits, bounded queues, timeouts, backpressure, interruptible script, quotas | Script loops, expansion bombs, recursive markup/CSS, large surfaces, storage fills, and request floods leave trusted UI responsive |
| Supply-chain or update compromise | Locked dependencies, provenance records, reviewed workflow changes, signed packages and metadata | Tampered package, wrong signer, replayed metadata, downgrade, expired metadata, key rotation, and interrupted update tests |
| History or private-session disclosure | Redacted diagnostics, opt-in telemetry, separate private state, crash-data policy | Inspect emitted logs/crash reports and disk state; sensitive URLs, page contents, cookies, and private profile data are absent by default |

Each implemented row needs an owner, executable test references, the exact tested
build/OS, and retained results. A prose control without a passing negative test
does not satisfy a release gate.

## Linux containment plan

Select a documented kernel and distribution baseline during bootstrap. Build a
small launcher that creates the required namespace and filesystem boundaries,
closes inherited descriptors, drops privileges, installs a role-specific syscall
policy, and verifies restrictions before handing over input. Keep child filesystem
views empty or limited to explicit read-only resources; provide fonts/assets via
brokered handles. Content and codec roles receive no network namespace access to
the host network and no profile directory mount. Network and storage roles obtain
only their required resources and cannot share the broker's full authority.

Evaluate namespaces, mount restrictions, seccomp-BPF, resource limits, and
available filesystem access controls as a composed design. The Linux kernel
documentation explicitly distinguishes syscall filtering from a full sandbox;
seccomp alone does not provide isolation or information-flow policy.
[Linux seccomp documentation](https://docs.kernel.org/userspace-api/seccomp_filter.html).

The prototype must test real distributions and their user-namespace policies,
including configurations where a required mechanism is unavailable. It must
fail closed with a specific diagnostic instead of silently weakening isolation.
File descriptor passing, shared memory, device access, JIT executable memory,
font lookup, accessibility IPC, and crash reporting need explicit role policies.

Treat renderer compromise as a design assumption and separate broker authority
from restricted target processes. Chromium documents that approach as a sandbox
design principle; BroWSER must independently demonstrate its implementation on
each supported OS. [Chromium sandbox design](https://chromium.googlesource.com/chromium/src/+/main/docs/design/sandbox.md).

macOS and Windows receive separate designs and negative tests before remote
browsing is supported on them. Building an offline developer binary on an OS
does not imply that its production sandbox is ready.

## IPC and broker policy

Define a schema with message type/version, bounded fields, request ID, capability
ID, and document generation. Treat every field as hostile. Reject unknown message
types, invalid enums, integer overflow, oversized allocations, invalid UTF-8 where
text is required, cycles in serialized structures, and invalid resource references
before acting. Validation and authorization happen in the receiving process.

Use a capability table private to the issuer. Capabilities encode permitted
operation, owning process/context, lifetime, and resource; avoid bearer strings
with unlimited scope. Revoke them on document teardown, process exit, permission
withdrawal, and profile change. Bind asynchronous replies to their initiating
generation. Cancellation does not permit a late result to alter the next page.

Share memory only for bounded bulk data, with validated lengths and offsets and
explicit read/write ownership. Validate a stable snapshot or enforce immutability
when a check could otherwise race a peer's modification. Never deserialize native
pointers, raw handles without broker validation, arbitrary paths, or executable
code into the broker. Malformed child traffic terminates the offending context
without taking down unrelated tabs or the UI.

## Network, document, and data policy

Remote loading requires tested URL/origin behavior, redirects, credentials,
cookies, CORS, response tainting, mixed-content restrictions, CSP, MIME handling,
and safe download treatment for the exposed feature set. These checks live at
their authoritative boundaries, not only in the renderer. The Fetch standard
defines the request/response and cross-origin model; BroWSER must integrate it
with document policy rather than regard an HTTP client as a browser network
stack. [Fetch standard](https://fetch.spec.whatwg.org/).

Treat `file:`, internal pages, downloads, external protocols, `data:`, and `blob:`
as separate policy cases. Initial offline fixtures are explicit input files with
no arbitrary filesystem traversal or network subresource loading. Do not grant
web content internal-page bindings, an unrestricted local-file origin, shell
execution, or automatic opening of downloaded executables.

Where an unimplemented capability affects isolation, disable its exposure as a
whole. For example, shared memory stays unavailable until cross-origin isolation,
process assignment, and side-channel review are complete. Early releases may
exclude workers, service workers, WebRTC, media capture, extensions, and persistent
authentication. Document those product restrictions and their enabling gates;
do not expose success-returning replacement APIs.

Persistent storage requires quotas, transaction/crash tests, partition checks,
clear-site-data behavior appropriate to the exposed APIs, and a tested profile
migration path. Private sessions avoid persistent state by construction; debug
logs, crash files, swap behavior, and OS services are considered in their privacy
documentation. “Private” is not presented as protection from a compromised host
or network anonymity.

## Security release gates

| Exposure level | Mandatory evidence | Release restriction |
| --- | --- | --- |
| Planning repository | Validated planning data, reviewed documents, no false implementation claims | No engine binary exists |
| Offline renderer | Deterministic fixtures, parser/layout fuzzing, input/decoded-size bounds, no ambient network or profile access | Explicit local test inputs only; no secure general browsing claim |
| Networked document viewer | Verified Linux process sandbox, hostile-child tests, origin/Fetch/TLS policies, isolated codecs for formats exposed, resource budgets, crash recovery | Only declared supported policies and formats; no scripting or unsafe embedding shortcuts |
| Scripted application browser | VM embedding/GC review, binding tests, interruption, task/microtask ordering, CSP, navigation teardown, cross-origin frame isolation or a documented blocked capability | Scripting enabled only in the reviewed supported configuration |
| Public general browsing candidate | Independent security review; vulnerability intake; patch/release owner; authenticated update/recovery tests; privacy/accessibility checks; complete release evidence | Block promotion for a known critical isolation failure or missing containment evidence |

These gates are cumulative. Any added capability reopens its relevant threat
review, fuzz targets, sandbox policy, and release evidence. Passing compatibility
or performance tests does not waive a security gate.

## Vulnerability handling and supply chain

Before distributing network-capable builds, enable private vulnerability
reporting on the repository, name a security release owner and backup, and test
the intake path. Keep reproduction material private when public disclosure would
expose users. Triage reports by impact, affected builds, exploitability, and
available mitigations; define response targets when staffing is assigned, rather
than publish service promises without an operator.

Maintain a dependency inventory with source, exact revision, license, owner,
security advisory source, and update procedure. Native codec, VM, cryptography,
and build dependencies receive the same attention as Rust packages. Apply
security updates on a rehearsed branch, run targeted regressions, and record the
shipped dependency versions. Do not fork cryptography to avoid an integration
problem.

Build releases from reviewed commits with pinned toolchains and dependency
locks. Separate signing credentials from untrusted pull-request workflows and
general build jobs. Generate checksums, dependency inventory, provenance, and a
release evidence manifest. The updater verifies authenticated metadata and
package hashes before installation and handles interruption atomically. Key
rotation and emergency rollback authorization need an exercised recovery plan.
None of these release mechanisms is represented as implemented in this repository.

## References and maintenance

Linked primary sources were consulted on 2026-11-16. Living standards and upstream
security mechanisms change; implementation work records the exact source/test
revision it targets and revisits this model when that revision, a platform
baseline, process boundary, or exposed capability changes. This document is a
decision baseline for implementation review, not a substitute for test evidence.
