# BAFT-TUNNEL Threat Model

**Status:** Active Phase 1 security baseline; Noise reverse-TCP requirements are specified in ADR-0009.

## Assets
- Node logical identities and Noise static X25519 transport private keys
- Enrollment tokens and signed desired-state documents
- Tunnel configuration, listener bindings, and routing metadata
- Panel credentials, audit records, and update-signing keys
- Availability and integrity of forwarded services

## Trust boundaries
1. Operator browser/CLI ↔ panel/API
2. Panel/API ↔ enrolled node agents
3. Node agent ↔ peer agent/data-plane endpoint over reverse TCP + Noise
4. Agent process ↔ host kernel, sockets, and service manager
5. Build/release pipeline ↔ signed artifacts

## Adversaries and risks
- Unauthenticated scanners and active protocol probing
- On-path observers, traffic classifiers, and network interference
- Compromised panel, node, operator workstation, or CI dependency
- Replay, downgrade, credential theft, static-key substitution, and malicious desired-state changes
- Resource exhaustion, amplification, malformed frames, and parser bugs
- Accidental operator changes or cleanup affecting unrelated host state

## Required mitigations
- Noise IK with pinned static X25519 peer keys, enrolled-key authorization, domain-separated prologue, and authenticated identity/tunnel/capability binding
- Fresh ephemeral keys and Noise handshake guarantees; reject wrong pins, unknown peers, altered prologues, downgrade attempts, and unsupported protocol versions
- Strict length-prefixed framing, maximum frame sizes before allocation, bounded queues/streams/sessions, handshake deadlines, and rate limits
- Short-lived scoped enrollment, transport-key rotation/revocation, and safe pin rollover
- Signed desired state with scope, monotonic generation, and expiry checks
- Least privilege, per-node/tunnel quotas, and anti-amplification controls
- Preview-before-apply, idempotency, receipts, explicit ownership of host resources, and rollback
- No payload logging; redact credentials and minimize operational metadata
- Reproducible builds, dependency review, published protocol-vector tests, fuzzing, signed releases, and independent security review

## Explicit limitations
Noise provides authenticated encryption; it does not guarantee invisibility or undetectability. Endpoint reputation, IP ownership, traffic volume, timing, packet sizes, connection patterns, correlation, active probing, and endpoint compromise remain observable risks. Padding, jitter, decoys, and port changes are mitigations to measure, not guarantees.

## Phase boundary
This threat model is not a security certification. Real cross-border LAB and production security claims remain NOT_RUN until independently evidenced.
