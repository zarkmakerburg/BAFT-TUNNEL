# ADR-0014: enrollment and control-plane trust boundary

**Status:** Accepted — offline operator-controlled enrollment for Phase 1; online enrollment deferred
**Decision date:** 2026-11-07

## Decision

Phase 1 will use the existing manual, offline-CA workflow to provision node identities. Online enrollment, a public enrollment endpoint, and a network-accessible CA signer are explicitly deferred to a later phase and are not Phase 1 release requirements.

This decision prioritizes a small, auditable trust boundary while the tunnel data plane and operator lifecycle are stabilized. It does not prevent a future control plane; it prevents that future service from becoming an unreviewed credential-minting authority in the first release.

### Phase 1 operational flow

1. The operator pre-authorizes a node identity and its intended role/tunnel configuration.
2. The node generates its private key locally and creates a CSR.
3. The CSR is transferred to the operator's offline CA environment through the documented/manual process.
4. The offline CA validates the request and issues a certificate with the authorized node identity (exactly one URI SAN: `urn:baft-tunnel:node:<node-id>`).
5. The operator securely transfers the node certificate and required public trust/revocation metadata back to the node. The private key never leaves the node.
6. The node is configured with its peer identity, trust anchor, certificate pin, and current CA-signed CRL. Runtime continues to fail closed on invalid, missing, expired, or revoked trust data.

The existing `scripts/pki/baft-pki.sh` helper is the implementation baseline. Any UX work must make this operator workflow understandable and difficult to misuse without moving CA secrets online.

### Security invariants

- The CA private key remains offline and encrypted; it must not be copied to a node, API process, control-plane database, CI artifact, or general-purpose application host.
- Node private keys are generated and retained on their respective nodes. Only CSRs and issued public certificates move between environments.
- The issuer determines the node identity; a CSR cannot self-select or override its authorized URI SAN.
- No public enrollment endpoint, online signer, automatic trust-on-first-use, or silent trust/pin replacement is authorized by this decision.
- CRL and certificate validation remain fail-closed. CRL distribution automation and safe CA/pin overlap/rollback remain separate acceptance work.
- No production host mutation is authorized by this ADR.

## Deferred online-enrollment design

The following is retained as a future design direction, not an implementation authorization. Any proposal to enable it requires a new owner-reviewed ADR or an explicit revision of this decision, plus independent security review.

### Trust boundary

Separate the system into three independently permissioned roles:

1. **Control-plane API:** authenticates operators, creates scoped enrollment grants, records node/tunnel policy, exposes signed desired state, and publishes public revocation metadata. It never receives node private keys or the CA signing key.
2. **Enrollment issuer:** validates CSRs and consumes one-time grants, then asks a separately protected CA signer to issue a node certificate. A networked signer requires a separate threat review and HSM/KMS decision.
3. **Node agent/data plane:** generates its key locally, proves possession through a signed CSR, receives a certificate and trust metadata, and continues to authenticate peers locally. Tunnel forwarding must not depend on a live control-plane request.

### Protocol requirements if reconsidered

- Cryptographically random, single-use, short-lived grants scoped to one pre-authorized node identity, role, and permitted tunnel set.
- Store only a one-way digest of high-entropy tokens; never log or expose plaintext after one-time creation.
- Node-generated private keys and CSR proof-of-possession. Strict validation of algorithms, extensions, identity, request size, and duplicate/malformed SANs.
- Server-controlled certificate identity and exactly one authorized node URI SAN.
- Atomic grant reservation, request/CSR binding, signer idempotency, crash-safe issuance receipts, and recovery reconciliation.
- Redaction, rate limits, non-enumerating errors, and auditable operator actions.
- Signed/versioned CRL publication, monotonic validation, atomic replacement, bounded staleness, and explicit degraded state.
- Explicit CA/pin overlap and rollback; no implicit trust-on-first-use.
- Control-plane outages must not weaken local data-plane authentication or authorization.

### Required gates before online enrollment

1. Select and independently threat-review the signer model; keep the offline signer as the default until a separately protected remote signer is justified.
2. Approve API authentication and operator authorization.
3. Approve persistence, uniqueness constraints, backup/restore, audit retention, and token rate limits.
4. Independently review protocol, concurrency, idempotency, crash-recovery, and revocation behavior.
5. Pass focused tests for grant scope/expiry/replay, concurrent redemption, CSR validation, signer failure/retry, crash recovery, redaction, CRL rollback/refresh, and control-plane outage isolation.

## Consequences

- Phase 1 can proceed without building an online enrollment service or introducing a network-accessible CA signing boundary.
- Initial server onboarding remains an operator task; documentation and UX should make it repeatable and safe.
- Online enrollment and its associated API, issuer, persistence, and threat model are deferred, not silently assumed complete.
- CRL automation, CA/pin rotation, operator-host credential custody/lifecycle, and independent protocol/security review remain separate work items.

