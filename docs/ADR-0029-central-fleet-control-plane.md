# ADR-0029: central Fleet control plane direction

**Status:** Proposed — owner-approved product direction; technical trust boundary and implementation remain gated  
**Decision date:** 2026-11-12  
**Related:** [ADR-0014](ADR-0014-online-enrollment-and-control-plane.md), [ADR-0023](ADR-0023-agent-control-helper.md), [ADR-0026](ADR-0026-panel-pairing-root-setup-helper.md), [ADR-0027](ADR-0027-panel-offline-tcp-tls-enrollment.md)

## Context

The current product provides a per-host loopback Agent and a dependency-free local web panel. Operators must access each node independently. There is no central Fleet inventory or control plane.

The owner approved the product direction of one central web panel, served over HTTPS, for managing multiple Iran/abroad nodes. The existing local panel must remain available as an independent emergency and local-management path.

This is a direction decision, not authorization to start Phase 2 implementation. The current MVP/B1–B6 delivery queue and Phase 1 gates remain ahead of Fleet implementation.

## Proposed direction

Build a central Fleet control plane with these boundaries:

1. **Central web application and API**
   - Provides authenticated operator access, Fleet inventory, node/tunnel views, health evidence, activity history and guided workflows.
   - Uses HTTPS for browser access.
   - Stores operator intent, node metadata, policy references, command/job state and audit records.
   - Does not store node private keys or the offline CA private key.

2. **Per-node Agent**
   - Retains the existing local API and local panel.
   - Establishes an outbound, mutually authenticated, encrypted control connection to the central service.
   - Does not require an inbound management port to be opened on the node.
   - Reconnects with bounded backoff and reports connection freshness separately from tunnel readiness.
   - Continues to operate the already-authorized local data plane if the central service is unavailable.

3. **Tunnel data plane**
   - Continues to authenticate peers and authorize forwarding locally.
   - Must not depend on a live central API, central database, heartbeat or browser session to keep an established tunnel forwarding.
   - Does not accept arbitrary shell commands or general-purpose remote execution from the control plane.

4. **Enrollment and identity**
   - Preserve ADR-0014: node keys are generated and retained on their nodes; the CA private key remains offline by default.
   - Initial Fleet registration must be compatible with operator-controlled, offline-issued node identities.
   - A central service must not silently become a CA signer or trust-on-first-use authority.
   - Any online enrollment grant issuer, remote signer, automated certificate issuance, trust-anchor replacement or pin rotation requires a separate owner-reviewed ADR and security review.

## Security and operational invariants

- **Outbound-only management path:** nodes initiate the control connection; the central service does not require public inbound management listeners on every node.
- **Mutual authentication:** both the node and central service authenticate the other using an explicitly provisioned trust relationship. The exact certificate profile, trust anchor and rotation mechanism remain open.
- **Least authority:** control messages are typed, versioned, bounded and allow-listed. No arbitrary command, shell, file path or executable payload is accepted.
- **Explicit mutation:** read-only inventory and diagnostics are separate from configuration changes and lifecycle actions. Mutating operations require a scoped authorization, an auditable request, an idempotency key, an expiry and a verifiable result. Whether each operation requires a separate operator confirmation is an open design decision.
- **Replay and ordering protection:** requests carry unique identifiers and monotonic or otherwise replay-resistant sequencing. Duplicate delivery must not repeat side effects.
- **Honest state:** Agent connectivity, last-seen time, reported service state, tunnel integrity evidence and end-to-end readiness are distinct fields. A heartbeat never implies a tunnel is healthy or Ready.
- **Local autonomy:** loss of the control plane cannot disable local authentication, revoke an already-authorized tunnel implicitly, or interrupt forwarding solely because the dashboard is offline.
- **Auditability:** operator identity, authorization decision, target node, requested operation, policy/version, result and timestamps are recorded. Secrets and forwarded payloads are never logged.
- **Secret custody:** node private keys remain node-local. The offline CA key is never placed in the central API, database, CI or general-purpose application host.
- **Safe degradation:** stale control-plane state is shown as stale/unknown; the Agent fails closed for new untrusted management actions but preserves already-authorized data-plane behavior.

## Proposed delivery slices (not yet authorized for implementation)

| Slice | Scope | Minimum acceptance |
|---|---|---|
| F0 — trust and protocol design | Threat model, identity lifecycle, channel protocol, central authority, storage and outage semantics | Owner-reviewed ADR; independent security review; abuse cases and protocol state machine |
| F1 — read-only Fleet | Node enrollment using existing offline-issued identity; inventory, last-seen and read-only diagnostics | Unauthenticated nodes rejected; no inbound node management port; freshness and Unknown semantics tested; no tunnel mutation |
| F2 — desired-state preview | Versioned desired-state model and per-node diff/validation | Invalid, stale, conflicting and unsupported state rejected; preview has no side effects |
| F3 — authorized operations | Typed jobs for explicitly supported setup/apply/start/stop operations | Scoped authorization, replay protection, idempotency, audit receipt, rollback/reconciliation and operator approval gates |
| F4 — operational history | Activity, alerts, node health evidence and operator audit views | Evidence freshness visible; no false Ready; bounded retention and redaction verified |
| F5 — resilience | Central outage, reconnect, duplicate delivery, upgrade and recovery behavior | Existing tunnels continue forwarding through control-plane outage; reconnect does not replay unsafe mutations |

No slice should be inferred as shipped from this proposal.

## Explicitly out of scope

- Replacing or removing the per-host local panel.
- Exposing the Agent's local API directly to the public Internet.
- Requiring inbound SSH or management ports for normal Fleet connectivity.
- Moving node private keys or the offline CA private key to the central server.
- Online CA signing, automatic trust-on-first-use or silent pin replacement.
- Arbitrary remote shell, command execution, file transfer or unrestricted proxying.
- Making the central service a dependency of tunnel forwarding.
- Billing, subscriptions, reseller/customer management or multi-tenant SaaS scope.
- Stable release, production deployment or real-host changes.

## Decisions still required before implementation

1. **Identity and trust:** exact node-to-control-plane and control-plane-to-node certificate profiles, trust-anchor provisioning, revocation and rotation; whether existing node identities can be safely reused for this distinct purpose.
2. **Enrollment:** operator-mediated bootstrap flow and one-time pairing ceremony compatible with the offline CA; no online signer is assumed.
3. **Control channel:** protocol framing, transport choice, version negotiation, limits, reconnect/backoff, keepalive and server-initiated message semantics.
4. **Authority model:** single-owner initial operation versus future roles; authorization scopes, approval boundaries and break-glass behavior.
5. **Desired state and jobs:** schema, generation/sequence semantics, idempotency, expiry, cancellation, rollback and reconciliation.
6. **Central persistence:** database choice, encryption, backup/restore, audit retention, migration and recovery.
7. **Deployment model:** central service packaging, domain/HTTPS termination, upgrades, monitoring and recovery.
8. **Offline behavior:** exact local-panel capabilities and how central/local changes are reconciled without overwriting newer authorized state.

## Gate and consequences

- This ADR records the owner's central-Fleet product direction only.
- Phase 2 implementation remains deferred until the current Phase 1/MVP queue and required B1–B6 gates are accepted, and F0 receives explicit owner approval after security review.
- The existing offline-CA and node-key-custody decisions remain binding unless separately revised.
- No code, runtime behavior, release, deployment, or host configuration is changed by this proposal.
- If accepted later, this direction enables one dashboard for multiple nodes while preserving local operation and data-plane autonomy.
