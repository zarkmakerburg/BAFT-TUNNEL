# ADR-0014: online enrollment and control-plane trust boundary

**Status:** Proposed; implementation blocked on security review and issuer selection
**Date:** 2026-11-07

## Context

Phase 1 currently bootstraps identities through a manual offline-CA workflow. Runtime TLS authenticates an already-issued certificate and binds its URI SAN to a configured node ID, but there is no online enrollment service, scoped authorization database, automated certificate lifecycle, or CRL distribution. Adding an HTTP endpoint without a deliberate trust boundary would turn enrollment into a credential-minting vulnerability.

The control plane must not become part of the tunnel data path. A control-plane outage must not silently weaken existing peer authentication or authorization.

## Proposed boundary

Separate the system into three independently permissioned roles:

1. **Control-plane API:** authenticates operators, creates scoped enrollment grants, records node/tunnel policy, exposes signed desired state, and publishes public revocation metadata. It never receives node private keys.
2. **Enrollment issuer:** validates CSRs and consumes one-time grants, then asks a separately protected CA signer to issue a node certificate. The CA signing key must not be mounted into the general API process or stored in the application database. Initial implementation should use the existing offline issuer adapter; a networked signer requires a separate threat review and HSM/KMS decision.
3. **Node agent/data plane:** generates its key locally, proves possession through a signed CSR, receives a certificate and trust metadata, and continues to authenticate peers locally. Tunnel forwarding must not depend on a live control-plane request.

## Enrollment protocol requirements

- An operator creates a cryptographically random, single-use, short-lived enrollment token scoped to exactly one pre-authorized node identity, allowed role, and permitted tunnel set.
- Persist only a one-way digest of the high-entropy token; never store or log its plaintext. Show the plaintext once at creation.
- The node generates its private key locally and submits a CSR over authenticated HTTPS. The issuer validates CSR proof-of-possession and rejects unsupported algorithms, malformed extensions, duplicate identity extensions, oversized requests, and unapproved SANs.
- The issuer, not the CSR, determines the certificate identity. The issued certificate contains exactly one URI SAN: urn:baft-tunnel:node:<authorized-node-id>.
- Grant redemption is an explicit state machine: unused → reserved → issued (or terminally failed/revoked). A transaction atomically reserves the grant for one request ID and CSR digest; concurrent requests cannot reserve it again.
- The signer interface must accept a stable idempotency key derived from the grant ID and request ID. If the API crashes after signing but before persisting the result, retrying the same reservation must retrieve the same signer result, never mint a second certificate. A different CSR or request ID cannot reuse the reservation.
- Persist an issuance receipt containing the request ID, CSR digest, certificate serial/fingerprint, signer result reference, and status. A client may retrieve the same completed result after a lost response without presenting or consuming the token again.
- Tokens are redacted from HTTP errors, logs, telemetry, support bundles, shell history, and URLs. Rate-limit by grant, source, and API principal. Responses must not reveal whether an arbitrary token exists.
- The CA signer returns the certificate and serial metadata only. Private keys never leave the node. The API database stores grant digests, node metadata, issuance receipts, and audit events—not private key material.

## Revocation, rotation, and outage behavior

- Publish CA-signed CRLs as versioned, cacheable public metadata. Nodes verify the CRL signature, issuer, validity interval, and monotonic version before atomic replacement; invalid or stale metadata never replaces a known-good CRL.
- CRL retrieval must remain possible when a node certificate is revoked. It therefore cannot require the very client certificate whose revocation the node must learn about; protect it with server-authenticated TLS, strict response validation, rate limits, and pinned trust.
- Keep the existing local fail-closed CRL behavior. Define a bounded maximum CRL age and an explicit operator-visible degraded/expired state; never silently continue as healthy with stale revocation data.
- CA and pin rotation must support an explicit overlap window: distribute the new trust anchor/pin, verify both old and new paths, issue replacement certificates, observe adoption, then revoke/remove the old path. No implicit trust-on-first-use or automatic pin replacement.
- Existing established tunnels may continue during a control-plane outage according to local policy and certificate/CRL validity. New enrollment, policy changes, and trust updates fail closed. A control-plane outage must not cause plaintext fallback or weaken mTLS.

## Persistence and concurrency requirements

- Use a transactional store with uniqueness constraints for token digests, node identities, certificate serials, and idempotency keys.
- Grant reservation and request binding must be one serialized database transition. The external signer call must be idempotent; API database transactions must not be held open across network calls.
- Recovery must reconcile reserved-but-not-completed requests against the signer's idempotency record. Concurrent redemption tests must prove one reservation and one certificate identity; crash-injection tests must cover failure before signing, after signing, and before receipt persistence.
- Store UTC timestamps, enforce expiry using server time, and reject clock-invalid or expired grants.
- Audit operator grant creation/revocation, enrollment attempts, issuance, certificate revocation, trust rotation, and desired-state changes. Audit records must not contain token plaintext, CSR private material, or secret-bearing request bodies.

## Required tests before enabling online enrollment

- Token entropy/format, digest-at-rest, expiry boundaries, wrong node/role/tunnel scope, revoked grant, and non-enumerating errors.
- Parallel redemption race: exactly one successful issuance; every other request rejected or given the same receipt only when the idempotency key and authenticated node binding match.
- CSR proof-of-possession, malformed/oversized CSR, unsupported algorithms, hostile/duplicate SANs, and server-controlled URI SAN issuance.
- Lost-response retry, signer idempotency, signer timeout/failure, database restart/recovery, and crash recovery across grant reservation, signing, and receipt persistence.
- Rate limiting, request-size limits, audit redaction, and no token leakage in logs/support bundles.
- CRL signature/issuer/expiry/rollback/version tests; revoked-node recovery path; atomic CRL replacement; CA/pin overlap and rollback tests.
- Control-plane outage tests proving existing data-plane policy remains local and no security downgrade occurs.

## Decision gates

Before implementation is enabled beyond isolated LAB:
1. Select and threat-review the CA signer model (offline manual signer first; HSM/KMS-backed remote signer only after separate review).
2. Select the API authentication and operator authorization model.
3. Approve database schema, backup/restore, audit retention, and token rate limits.
4. Review the protocol and concurrency tests independently.

This ADR does not authorize exposing an enrollment endpoint publicly, moving CA keys online, or changing production hosts.
