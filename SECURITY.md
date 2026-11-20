# Security policy

There are no supported browser releases. The M1 development bootstrap handles only
controlled synthetic offline fixtures and is unsandboxed; arbitrary web browsing is
not available. Future releases need the gates in
[the security model](docs/SECURITY_MODEL.md) before support is declared.

Report sensitive findings through
[GitHub private vulnerability reporting](https://github.com/pookie421/BroWSER/security/advisories/new)
when enabled for this repository. If GitHub reports that private reporting is
unavailable, open a public issue requesting a private reporting channel, without
exploit details, private data or proof-of-concept attachments. A maintainer must
establish that channel before receiving those details. No monitored private email
address or response-time service is claimed by this baseline.

Include the affected commit, environment, safe reproduction steps, impact and any
existing workaround in the private report. Do not use real user profiles or secrets
in a reproducer. Maintainers will agree on disclosure and track the fix with a
regression test. There is no funded bug bounty at this stage.
