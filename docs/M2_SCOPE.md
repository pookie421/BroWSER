# M2 acceptance candidate

20 November 2026 · For engineering and security review before the cohort is
frozen. M2 and B020–B024 remain planned in the [roadmap](../planning/roadmap.json).
Repository validation and the Linux and macOS engine jobs passed on `c97fa6a`.
Those jobs exercise the offline M1 bootstrap. [GitHub Actions run 44714958909](https://github.com/pookie421/BroWSER/actions/runs/44714958909)

M2 targets a controlled internal network document viewer on Linux x86-64. It
builds on the M1 renderer, keeps scripting disabled, and gives the broker
authority over navigation, origin, top-level partition and child capabilities.
The [security model](SECURITY_MODEL.md) and [validation policy](VALIDATION.md)
define the mandatory controls and evidence.

## Offline broker experiment

```sh
cargo test --locked --offline --example m2_broker
cargo run --locked --offline --example m2_broker
```

The [example](../examples/m2_broker.rs) spawns two copies of itself with separate
anonymous pipe pairs. The broker supplies the child identity from the process it
connected to. It validates the frame version, operation and fixed size before
reading the body. It checks the capability and current document generation
before sending four synthetic resource bytes. The other child presents the same
capability and receives a denial. The authorized child still receives the
resource, then receives a denial after the broker advances the document and
revokes the grant.

The supervisor gives the pipe exchange and child exits one five-second
deadline. It terminates and reaps both children on failure. The fixture checks
fixed response bytes, with wire-format tests for grants and denials and a
subprocess test that verifies reaping after a child I/O timeout. The subprocess
replay runs in Ubuntu and macOS CI.

This experiment exercises a single broker record and predefined workers.
Production IPC still needs bounded peer lifetime and queues, request tracking,
broker-owned origin and partition data, teardown revocation, shared buffer
validation and Linux worker containment. The browser executable remains
offline. The example is outside the candidate test denominator.

## Candidate test groups

Each group needs exact case IDs, inputs and independent expected results before
review. Include a valid control alongside each class of rejection so a system
that rejects all requests fails the cohort.

| Candidate group | Task | Required observations |
| --- | --- | --- |
| IPC-01 | B020 | A supported message within bounds reaches its intended context. Unknown schema values, invalid lengths/offsets and reused request IDs within a context and generation fail before the requested operation runs. |
| IPC-02 | B020 | A request with the wrong owner, operation or generation cannot use or extend a capability. Teardown revokes authority. Validate stable shared-buffer bytes and confirm that concurrent changes cannot alter an authorized operation. One rejected peer leaves the other context responsive. |
| OS-01 | B021 | Renderer and codec workers start with verified role restrictions and process one permitted synthetic job. They cannot access ambient files, inherited handles, sockets, devices, profiles, unrelated contexts or general desktop services. |
| OS-02 | B021 | Startup with unavailable required isolation fails before input delivery. A worker exit or a bounded decode failure leaves the broker and another tab responsive. |
| NET-01 | B022 | Pinned URL cases produce canonical URLs and the correct origins, including same host with different schemes or ports. Controlled HTTPS succeeds with the trusted fixture CA. Untrusted, wrong-name and expired certificates fail before a page commits. Exercise the declared HTTP and mixed-content policies. |
| NET-02 | B022 | Check authorized and denied same-origin and cross-origin requests, CORS, response tainting, CSP for supported resources and MIME handling. Each redirect revalidates the destination while preserving initiating authority and partition. A cancelled or over-budget request cannot publish a usable response. |
| NET-03 | B022 | Two origins across two top-level partitions and normal/private profiles receive only their allowed credentials, cookies and cache entries. Verify permitted same-partition reuse and denied cross-partition reuse. |
| NAV-01 | B023 | Trusted UI shows the committed origin from broker state. Replay redirects, back/forward, replacement, cancellation and delayed responses. Obsolete responses cannot change the newer page, its origin indicator or history. |
| NAV-02 | B023 | Test explicit file, internal, opaque and unsupported embedding policies. Verify bounded downloads, private-session teardown, persistent partition and quota invariants, and unaffected-tab navigation after a renderer crash. |
| REVIEW-01 | B024 | An independent reviewer replays mandatory groups on a fresh supported runtime, checks codec containment and process teardown, and records disposition and retest evidence for all findings. No critical or high finding remains open at gate approval. |

## Freeze and replay

- **Runtime.** Start evaluation on Ubuntu 24.04 x86-64, the Linux CI family
  already used for M1. Freeze an immutable worker image, exact kernel and
  effective namespace, syscall and filesystem policy. Record executable, build
  flags, worker identity, installed controls and the intentionally unsupported
  configuration used for the fail-closed check. Ubuntu 24.04 mediates
  unprivileged user namespaces through AppArmor, so the worker must exercise
  the intended launcher profile with the host restriction active. A macOS build
  supplies development evidence. [Ubuntu AppArmor documentation](https://documentation.ubuntu.com/security/security-features/privilege-restriction/apparmor/#apparmor-unprivileged-user-namespace-restrictions)
- **Inputs.** Use synthetic documents, one ephemeral fixture profile per run,
  controlled origins and deterministic response schedules. Store the test
  certificate identities, trust fixture and verification time with the corpus.
  Freeze fixture and independent reference hashes, relevant upstream revisions
  and exact case selections. Record distinct pre-limit, at-limit and over-limit
  cases for request, response, decode, download, queue, time and storage budgets.
- **Oracles.** Assert the expected protocol decision, receiving process, broker
  state, origin and partition, and allowed changes to files or stores. For a
  denial, verify the required absence of state changes or data delivery, then
  run a valid control. Gate results on observed process behavior and trusted
  state, including when a peer exits or a response arrives out of order.
- **Reports.** Freeze cases and commands before running them. Record the tested
  build, runtime, corpus, tool and harness identities, exact commands, elapsed
  time and sanitized artifact hashes. Publish the full denominator with distinct
  pass, fail, timeout, crash, harness-error, expected-failure, unsupported,
  not-run and pending manual review outcomes. Only passes count toward the gate.
  Retain sanitized evidence immutably and document independent replay and
  review against the exact candidate commit.

## Implementation order

1. **During M1.** Draft the corpus and review the Linux baseline. Keep any IPC
   and containment feasibility experiments isolated, with synthetic data and
   without a browsing entry point. B019 and M1 still gate M2 delivery.
2. **After B019.** Deliver the B020 broker context and IPC boundary first. Once
   B020 is accepted, run B021 containment and B022 network policy in parallel.
   Before containment acceptance, use synthetic request/response inputs without
   delivering network content to a renderer or codec. With verified containment,
   exercise controlled fixture origins and test certificates before expanding
   exposure.
3. **After B021 and B022.** Integrate B023 navigation, trusted origin display,
   storage and crash recovery. Close B024 through independent replay and review
   before arbitrary public content becomes eligible even for internal use. M2
   and M3 remain internal test stages. External network-capable distribution
   additionally requires the applicable B042 operating and release controls.
4. **Parallel work.** Continue B030 VM feasibility in isolation, with real builds,
   GC ownership checks and an update rehearsal. B031 needs both B030 and B024.
   Capture dependency provenance and the release rehearsal inputs as components
   land. After B024, run B042 signed-update, recovery and incident rehearsals
   alongside M3 work. [VM feasibility](VM_FEASIBILITY.md) · [Backlog](BACKLOG.md)
