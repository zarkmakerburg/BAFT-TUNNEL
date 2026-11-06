# BAFT-TUNNEL Threat Model

**Status:** Phase 0 baseline; requires review before implementation.

## Assets
- Node identity and transport private keys
- Enrollment tokens and signed desired-state documents
- Tunnel configuration, listener bindings, and routing metadata
- Panel credentials, audit records, and update-signing keys
- Availability and integrity of forwarded services

## Trust boundaries
1. Operator browser/CLI ↔ panel/API
2. Panel/API ↔ enrolled node agents
3. Node agent ↔ peer agent/data-plane endpoint
4. Agent process ↔ host kernel, sockets, and service manager
5. Build/release pipeline ↔ signed artifacts

## Adversaries and risks
- Unauthenticated scanners and active protocol probing
- On-path observers, traffic classifiers, and network interference
- Compromised panel, node, operator workstation, or CI dependency
- Replay, downgrade, credential theft, and malicious desired-state changes
- Resource exhaustion, amplification, malformed frames, and parser bugs
- Accidental operator changes or cleanup affecting unrelated host state

## Required mitigations
- TLS 1.3 mutual authentication, pinned node identity, short-lived scoped enrollment, rotation and revocation
- Fresh nonces, replay protection, protocol version negotiation, and downgrade rejection
- Signed desired state with scope, monotonic generation, and expiry checks
- Least privilege, bounded buffers, per-node/tunnel quotas, rate limits, and anti-amplification controls
- Preview-before-apply, idempotency, receipts, explicit ownership of host resources, and rollback
- No payload logging; redact credentials and minimize operational metadata
- Reproducible builds, dependency review, signed releases, and independent security review

## Explicit limitations
The product cannot guarantee invisibility or undetectability. Endpoint reputation, IP ownership, traffic volume, timing, packet sizes, TLS metadata, correlation, active probing, and endpoint compromise remain observable risks. Padding, jitter, decoys, and port changes are mitigations to measure, not guarantees.

## Phase boundary
This threat model is a starting point, not a security certification. No real tunnel or production security claim is authorized by Phase 0.
