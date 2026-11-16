# Implementation backlog

The canonical contracts and dependency graph are in [planning/roadmap.json](../planning/roadmap.json). Only B001 and M0 concern this planning package; every engine task is planned. P0 blocks correctness, security or its milestone; P1 expands required product coverage. Role names are staffing requirements.

Task dependencies are completion dependencies. B030 can run during M1 because VM feasibility must retire architectural risk early; it does not enable scripts before M2 security approval. Release operations can start after M2 while runtime work proceeds.

M2 and M3 are internal test builds. Any external distribution of a network-capable
build also requires B042's applicable update, intake, response and recovery gates;
the later milestone label does not permit earlier distribution without those controls.

| ID | Work item | Gate | Priority | Depends on |
| --- | --- | --- | --- | --- |
| B001 | Publish the verified engineering baseline | M0 | P0 | None |
| B010 | Freeze the M1 capability and fixture contract | M1 | P0 | B001 |
| B011 | Bootstrap reproducible Rust builds and a real executable slice | M1 | P0 | B010 |
| B012 | Implement the HTML tokenizer cohort | M1 | P0 | B011 |
| B013 | Implement DOM ownership and HTML tree construction | M1 | P0 | B012 |
| B014 | Implement CSS syntax, selectors and cascade | M1 | P0 | B011 |
| B015 | Integrate deterministic international text metrics | M1 | P0 | B011 |
| B016 | Implement block and inline formatting | M1 | P0 | B013, B014, B015 |
| B017 | Implement display lists, PNG resources and CPU paint | M1 | P0 | B016 |
| B018 | Deliver the offline fixture runner and inspectable output | M1 | P0 | B017 |
| B019 | Close the M1 renderer gate with independent replay | M1 | P0 | B018 |
| B020 | Implement broker-owned context identity and capability IPC | M2 | P0 | B019 |
| B021 | Enforce the Linux content and codec sandbox | M2 | P0 | B020 |
| B022 | Implement URL, Fetch, TLS and partitioned network policy | M2 | P0 | B020 |
| B023 | Deliver navigation, trusted origin UI and storage lifecycle | M2 | P0 | B021, B022 |
| B024 | Close the network security gate | M2 | P0 | B023 |
| B030 | Select and prove a maintainable JavaScript VM embedding | M3 | P0 | B011 |
| B031 | Implement traced Web IDL and DOM bindings | M3 | P0 | B024, B030 |
| B032 | Integrate the HTML event loop and dynamic rendering | M3 | P0 | B031 |
| B033 | Close the dynamic platform compatibility gate | M3 | P0 | B032 |
| B040 | Expand layout, forms, editing and international interaction | M4 | P1 | B033 |
| B041 | Deliver accessibility, inspector and WebDriver integration | M4 | P0 | B040 |
| B042 | Establish signed release, update and incident operations | M4 | P0 | B024 |
| B043 | Close the developer alpha gate | M4 | P0 | B041, B042 |
| B050 | Broaden standards, site compatibility and platform coverage | M5 | P1 | B043 |
| B051 | Demonstrate competitive performance with reproducible controls | M5 | P1 | B043 |
| B052 | Complete independent release security and continuity review | M5 | P0 | B050, B051, B042 |

## Completion evidence

Every task contains explicit acceptance criteria in the JSON plan. Attach a committed evidence index containing exact commands, commit/tool/corpus versions, output hashes, failures and reviewer decisions. Large sanitized raw outputs may live in immutable release artifacts linked by that index. Planned evidence paths are contracts, not files to pre-create with empty content.

A file existing does not prove a task is done. The validator enforces structural integrity and file references; human review evaluates evidence quality and the actual gates in [ROADMAP.md](ROADMAP.md). Never mark engine work complete using a planning document as its test result.

When a task is split, preserve the original acceptance obligation and add explicit dependencies. A security or conformance requirement cannot disappear by moving it to an unowned future item. Add fully specified tasks when the next milestone scope is frozen; this initial list covers the major delivery contracts without pretending to enumerate every Web API.
