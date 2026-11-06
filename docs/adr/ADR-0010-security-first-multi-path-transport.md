# ADR-0010: Security-first multi-path transport architecture

**Status:** Accepted by owner direction on 2026-11-06; supersedes ADR-0009.
**Supersedes:** ADR-0009 as the Phase 1 data-plane transport decision.
**Scope:** Node-to-node data plane, connection establishment, transport selection, and security boundaries.

## Context

BAFT-TUNNEL must support heterogeneous networks, including paths where UDP is unavailable or unreliable. It must provide strong authenticated encryption, explicit node authorization, resilient connectivity, and measured privacy properties. No transport can guarantee that traffic is indistinguishable from other traffic or cannot be blocked. Encryption, metadata privacy, traffic classification resistance, and availability are separate properties and must be evaluated separately.

The earlier Noise-first decision was an owner-directed requirement, but the engineering review identified no demonstrated security or operational advantage sufficient to make an unaudited Noise implementation the mandatory production transport while a mature TLS 1.3 implementation and ecosystem are already available. The existing `snow` crate reports no formal security audit. Noise does not inherently conceal traffic metadata or prevent protocol classification.

## Decision

### 1. Separate connectivity mode from secure transport

Direct, Reverse, and Relay are connection/topology modes, not cryptographic protocols:

- **Direct:** either node may dial the peer's reachable listener.
- **Reverse:** a node behind NAT/firewall initiates an outbound connection to a reachable peer/hub; logical reverse streams use that established session.
- **Relay:** an intermediary forwards opaque, end-to-end protected traffic when direct paths are unavailable.

The connection manager may probe and select among explicitly authorized paths. It must not infer reachability from ICMP alone. Path selection uses authenticated connection establishment and bounded bidirectional health checks. Auto-selection must never weaken authentication or encryption.

### 2. Phase 1 baseline: TCP + TLS 1.3 mutual authentication

- Use TLS 1.3 only for the initial TCP data-plane adapter, with mutual authentication.
- Use per-node identities and a private enrollment/issuance trust model; bind certificate identity to the enrolled logical node and authorization record.
- Apply strict certificate validation, trust-anchor constraints, revocation/rotation strategy, and endpoint authorization. Authentication is not authorization.
- Do not implement custom cryptography, custom TLS fingerprints, or plaintext/legacy fallback.
- Support both Direct and Reverse using the same secure TCP adapter; only the dial/listen orchestration differs.
- Retain bounded Yamux multiplexing over the authenticated TLS stream for TCP sessions, with strict limits and documented per-session TCP head-of-line blocking.
- TLS 1.3 early data (0-RTT) is disabled for tunnel data and all non-idempotent/control operations unless a separately reviewed replay-safe design proves it safe.

TLS 1.3 is specified by RFC 8446. ECH is specified separately by RFC 9849 and is an optional privacy enhancement, not a transport and not a guarantee of concealment.

### 3. QUIC is a later optional transport

- QUIC over UDP with TLS 1.3 and native QUIC streams is a planned additional adapter, not a Phase 1 prerequisite.
- Do not assume UDP availability. TCP remains an independent supported path.
- Direct and Reverse topology modes may use QUIC when authenticated reachability and policy permit.
- Do not put Yamux on top of QUIC; use native QUIC streams.
- Evaluate congestion control, migration, anti-amplification, resource limits, and real network compatibility before enabling it by default.

### 4. Noise is not the mandatory Phase 1 production transport

- The current Noise IK prototype may remain isolated for comparative tests and research.
- Do not route production/runtime tunnel traffic through Noise until an explicit ADR demonstrates a concrete advantage, completes independent cryptographic review, validates protocol vectors/interoperability, and closes the dependency audit/fuzzing gates.
- No silent negotiation or downgrade between TLS and Noise. Any future alternate transport must be explicitly versioned and policy-authorized.

### 5. Privacy and traffic observability

Security, privacy, traffic classification resistance, and censorship/availability are measured independently:

- Minimize exposed identity and metadata in application messages, logs, metrics, and support bundles.
- Never log payloads, credentials, private keys, enrollment tokens, or sensitive endpoint configuration.
- ECH may be evaluated where DNS and server deployment support it; it protects selected ClientHello fields only and does not hide destination IP, timing, volume, or all traffic characteristics.
- Do not claim traffic is invisible, undetectable, or censorship-resistant. Document observable metadata and test privacy properties against a stated observer/threat model.
- Any traffic-shaping or padding feature requires a documented privacy benefit, bandwidth/latency cost, abuse analysis, and explicit opt-in; it must not be presented as a guarantee.

### 6. Relay and end-to-end security

- Relay is a topology mode, not a trusted plaintext termination point.
- When relay is introduced, preserve end-to-end authentication/encryption between the two tunnel endpoints; relay authorization and transport security are additional layers, not substitutes.
- Bound relay sessions, queues, bandwidth, and lifetime; prevent open-relay behavior.

## Required implementation and acceptance

1. Replace the mandatory Noise runtime target with TCP + TLS 1.3 mTLS for Phase 1; keep Direct and Reverse as separate connection strategies over the same adapter.
2. Ensure certificate identities map to enrolled nodes and explicit tunnel/stream authorization.
3. Add direct and reverse two-process loopback E2E tests over the same secure TCP adapter.
4. Test invalid/expired/revoked/untrusted certificates, wrong node identity, wrong tunnel authorization, replay-sensitive operations, malformed framing, fragmented/coalesced frames, timeouts, resource exhaustion, concurrent streams, cancellation, drain, reconnect, and session isolation.
5. Preserve strict frame/stream/session/queue bounds and explicit behavior when a TCP carrier fails; do not claim transparent migration of established TCP streams.
6. Complete dependency advisory/license review, fuzzing of protocol boundaries, and independent security review before production readiness.
7. Evaluate QUIC separately with UDP-disabled fallback tests and real-host measurements.
8. Evaluate ECH as a privacy enhancement only; test supported DNS/TLS deployment and document remaining metadata exposure.
9. Real six-host cross-border LAB remains NOT_RUN until the owner supplies server availability and access.

## Consequences

- ADR-0009 is superseded for the Phase 1 production data plane. Its Noise implementation becomes non-production research code pending separate review.
- The existing TLS/mTLS implementation becomes the Phase 1 baseline, subject to identity/authorization hardening and the tests above.
- This decision does not authorize merge, release, deployment, production-host changes, or real-host LAB execution.
- No guarantee of traffic invisibility or resistance to network blocking is made.
