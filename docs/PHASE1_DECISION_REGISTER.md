# Phase 1 Decision Register — Offline Review

**Status:** Open decision register; no implementation authorization.
**Applies to:** Phase 1 authenticated TCP forwarding vertical slice.
**Authority:** Owner decisions are required for security-sensitive choices. This register does not approve Phase 1, change the phase gate, or authorize host access.

This register turns the unresolved items in `PHASE1_EXECUTION_PLAN.md` into reviewable decisions. Do not silently convert a proposal or library default into a product guarantee.

## Decision rules

- Record the decision, rationale, rejected alternatives, reviewer, date, and affected protocol/config version.
- Security and compatibility decisions must be made before implementation depends on them.
- Prefer maintained, audited ecosystem components over custom protocol/cryptographic code.
- If evidence is insufficient, mark **BLOCKED / UNKNOWN**; do not select by popularity alone.
- Any change after implementation begins requires an explicit compatibility and migration assessment.

## Open decisions

| ID | Decision | Required evidence before selection | Consequence if unresolved | Gate |
|---|---|---|---|---|
| D1 | Rust stream-multiplexing library | Current maintenance activity; license compatibility; Tokio integration; flow control; bounded memory; cancellation/drain semantics; security advisories; interoperability tests | No wire framing or engine implementation can be safely fixed | Before P1.1 exit |
| D2 | Node identity and transport credential model | Threat-model review; certificate issuance/rotation/revocation lifecycle; key storage options; identity pinning semantics; recovery procedure | Authentication and trust bootstrap remain underspecified | Before P1.2 |
| D3 | Handshake and wire compatibility | Version negotiation rules; transcript binding; nonce freshness; downgrade rejection; unknown capability behavior; malformed-input limits | Peers may disagree on identity, capabilities, or protocol safety | Before P1.2 |
| D4 | Replay protection durability | Identify replay-sensitive effects; crash/power-loss model; persistence atomicity; retention and expiry; recovery tests | Replay-sensitive side effects must remain disabled | Before any such side effect |
| D5 | Initial peer configuration/bootstrap | Compare static local configuration with minimal enrollment; assess operator burden, secret exposure, recovery and revocation | No automatic enrollment or remote provisioning | Before P1.3 |
| D6 | Supported Linux baseline | Target distributions/versions; kernel and systemd minimums; architecture matrix; package availability | Installer support claims cannot be made | Before P1.5 |
| D7 | Installer privilege boundary | Exact files, users, directories and systemd operations; required capabilities; rollback ownership proof; non-root runtime feasibility | No privileged installer mutation | Before P1.5 |
| D8 | TCP listener and destination policy | Bind-address default; loopback vs public exposure; destination allow/deny policy; DNS rebinding considerations; port collision behavior | No listener opens until explicit validated configuration | Before P1.3 |

## Required decision record template

For each D-ID, record:

1. **Decision:** one unambiguous statement.
2. **Status:** proposed / accepted / rejected / blocked.
3. **Evidence:** dated links, versions, test results, licenses and advisories.
4. **Security impact:** confidentiality, integrity, availability, privilege and recovery.
5. **Compatibility:** wire/config version impact and migration behavior.
6. **Alternatives:** viable options and why they were not selected.
7. **Approval:** named owner/reviewer and date.

## Pre-implementation stop conditions

Stop and return to review if any of the following is true:

- A candidate requires custom cryptography or silently weakens TLS/authentication.
- Resource bounds, backpressure, cancellation, or graceful shutdown cannot be demonstrated.
- Peer identity can be accepted based only on a network address or an unverified name.
- A security-sensitive action can occur before replay state is durable.
- Installer cleanup cannot distinguish BAFT-TUNNEL-owned resources from pre-existing host state.
- Logs, metrics, crash reports, or support bundles can expose secrets or forwarded payloads.
- A status can report healthy without fresh evidence from the actual forwarding path.

## Current disposition

All D1–D8 remain **OPEN / NOT DECIDED**. No library, certificate lifecycle, wire format, distribution support matrix, installer privilege model, or public-listener default is approved by this document.

The six-host LAB remains unavailable and real-host tests remain **NOT RUN**. Phase 1 remains **NOT AUTHORIZED**. PR #1 remains Draft.