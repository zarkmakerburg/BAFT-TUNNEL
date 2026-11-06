# BAFT-TUNNEL Threat Model

**Status:** Active Phase 1 security baseline. ADR-0010 selects TCP + TLS 1.3 mutual authentication; ADR-0009's Noise-first direction is superseded for the Phase 1 production data plane.

## Assets
- Enrolled logical node identities and TLS transport private keys
- Enrollment tokens, trust anchors, certificate lifecycle and signed desired-state documents
- Tunnel configuration, listener bindings, authorization policy, and routing metadata
- Panel credentials, audit records, and update-signing keys
- Availability and integrity of forwarded services

## Trust boundaries
1. Operator browser/CLI ↔ panel/API
2. Panel/API ↔ enrolled node agents
3. Node agent ↔ peer agent/data-plane endpoint over TCP + TLS 1.3 mTLS
4. Agent process ↔ host kernel, sockets, and service manager
5. Build/release pipeline ↔ signed artifacts

## Adversaries and risks
- Unauthenticated scanners and active protocol probing
- On-path observers, active attackers, traffic classifiers, and network interference
- Compromised panel, node, operator workstation, or CI dependency
- Replay, downgrade, credential theft, certificate/key substitution, identity spoofing, and malicious desired-state changes
- Resource exhaustion, amplification, malformed frames, and parser bugs
- Accidental operator changes or cleanup affecting unrelated host state

## Required mitigations
- TLS 1.3 only with mutual certificate authentication, strict trust-anchor validation, and no plaintext or legacy fallback
- Bind each certificate to exactly one enrolled node identity using a validated SAN identity (URI form defined by the enrollment contract); never trust node IDs merely claimed in the application handshake
- Authorize the authenticated node for the requested tunnel, direction, and stream capability; authentication alone is insufficient
- Short-lived scoped enrollment, secure private-key storage, certificate rotation/revocation, and safe trust/pin rollover
- Disable TLS 1.3 early data for forwarded traffic and non-idempotent/control operations unless a reviewed replay-safe design is approved
- Strict length-prefixed framing, maximum frame sizes before allocation, bounded queues/streams/sessions, handshake deadlines, and rate limits
- Signed desired state with scope, monotonic generation, and expiry checks
- Least privilege, per-node/tunnel quotas, and anti-amplification controls
- Preview-before-apply, idempotency, receipts, explicit ownership of host resources, and rollback
- No payload logging; redact credentials and minimize operational metadata
- Reproducible builds, dependency review, published protocol-vector tests, fuzzing, signed releases, and independent security review

## Explicit limitations
TLS 1.3 mTLS provides authenticated encrypted transport when certificate validation and identity authorization are correctly implemented. It does not guarantee invisibility or undetectability. Endpoint reputation, IP ownership, traffic volume, timing, packet sizes, connection patterns, correlation, active probing, and endpoint compromise remain observable risks. ECH, padding, jitter, and traffic shaping are optional mitigations to evaluate, not guarantees.

## Phase boundary
This threat model is not a security certification. The Phase 0 two-host host-inspection preflight succeeded, but Phase 1 authenticated tunnel E2E, security hardening, and production readiness remain NOT_RUN until independently evidenced.
