# ADR-0009: Reverse TCP transport secured by Noise

**Status:** Accepted by owner direction on 2026-11-06; implementation required in Phase 1.
**Supersedes:** The Phase 0 proposal to use TLS 1.3 mutual authentication for the node-to-node data plane.
**Scope:** Node-to-node data plane only. HTTPS for the panel/API remains TLS.

## Decision

The mandatory Phase 1 data path is **reverse TCP + Noise**, not TLS-wrapped TCP:

1. The restricted/remote-side agent is the TCP dialer. It opens and maintains outbound TCP connections to the reachable hub/listener; the design must not require inbound reachability at that agent.
2. The hub accepts those connections. The direction is deliberate: the agent behind NAT/firewall dials outward and receives reverse logical streams over the established session.
3. Authenticate and encrypt the raw TCP byte stream with the Noise Protocol Framework using the reviewed Rust `snow` implementation. Initial pattern/suite: `Noise_IK_25519_ChaChaPoly_BLAKE2s`, subject to dependency and protocol-vector verification before the implementation is declared complete.
4. Pin each peer's Noise static X25519 public key and bind the authenticated key to the enrolled logical node identity. Do not treat a claimed node ID as authentication.
5. Bind protocol name/version, both logical node IDs, tunnel ID, role, and negotiated capabilities into the Noise handshake prologue and/or authenticated handshake payload. Reject mismatches and unsupported versions.
6. Run bounded Yamux logical streams over a framed Noise transport adapter. Keep independent TCP sessions in a bounded pool to contain per-connection TCP head-of-line blocking.
7. Use bounded reconnect with jitter, admission limits, keepalive, graceful drain, and explicit behavior for streams lost with a TCP session. Do not claim transparent migration of established TCP streams.

## Key and protocol requirements

- Generate transport static keys from a cryptographically secure RNG; persist with restrictive permissions and never log or include private material in support bundles.
- The initiator must know and pin the responder static public key before starting IK. The responder must validate the initiator static key against the enrolled peer record after the handshake.
- Use a fixed, versioned protocol prologue for domain separation. No silent fallback to TLS, plaintext, another Noise pattern, or a weaker suite.
- Add explicit length-prefixed framing with strict maximum frame sizes before allocation; handle partial TCP reads/writes correctly.
- Use Noise transport-state nonces exactly once and follow the library's rekey guidance; close a session on authentication, framing, nonce, or state errors.
- Authenticate all control metadata; do not rely on unauthenticated pre-handshake identity labels.
- Rotation/revocation must be designed around peer pin updates and safe overlap; enrollment and key lifecycle remain required work, not implied by static pinning.
- The existing TLS data-plane implementation is transitional and must be removed from the node-to-node path once Noise passes the same or stronger conformance tests. Keep rustls for HTTPS/API if needed.

## Required tests / acceptance

- Published Noise test vectors and deterministic handshake tests for the selected pattern/suite.
- Positive IK handshake and transport round-trip; wrong responder pin, unknown initiator key, altered prologue, wrong node/tunnel binding, and malformed/oversized frame rejection.
- Replay/reconnect/session isolation tests; transport nonce and rekey boundary tests supported by the library.
- Fragmented TCP reads/writes, coalesced frames, truncation, EOF, slowloris timeout, and resource exhaustion tests.
- Yamux multiplexing over the Noise adapter; backpressure, cancellation, stream cap, pool cap, drain, and reconnect behavior.
- Two-process loopback reverse-TCP end-to-end test before six-host LAB.
- Real cross-border LAB remains NOT_RUN until the owner supplies server availability and access.

## Consequences

- The current TLS/mTLS implementation cannot be considered the final Phase 1 transport.
- The Noise stream adapter and key lifecycle are explicit P0 implementation work.
- Noise provides authenticated encryption, not traffic invisibility or censorship resistance. Network metadata, timing, volume, and endpoint behavior remain observable.
- No production host mutation, merge, release, or deployment is authorized by this ADR.
