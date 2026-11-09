# BAFT-TUNNEL Architecture

**Status:** Phase 1 security-first multi-path baseline; see ADR-0010.

## Product boundary

BAFT-TUNNEL is an independent Linux server-to-server TCP/UDP forwarding platform. It supports Direct and Reverse connection establishment, with Relay as a later topology option. Inbound reachability at both endpoints is never assumed. The product is not a subscription panel; its API is intended for integration with third-party panels.

No BAFT implementation, history, keys, production configuration, CI secrets, or deployment assets are imported.

## Components

- **Node agent:** maintains authenticated transport sessions, listeners, forwarding sockets, health probes, and reconciliation.
- **Panel/API:** enrollment, node identity, desired state, policy, audit, and operator UI.
- **CLI:** diagnostics, enrollment, plan/apply, backup/restore, and support bundle.
- **Connection manager:** evaluates authorized Direct/Reverse/Relay paths and selects a healthy authenticated route.
- **Transport adapters:** versioned, replaceable TCP and later QUIC data-plane implementations.
- **Web UI:** plain JavaScript with no build step, served by the agent; Persian-first RTL with English support.

The core engine, agent, API and CLI are Rust; the installer and PKI helpers are shell scripts. Tokio is the async runtime; rustls is used for TLS 1.3 transport. The agent API is a small loopback HTTP server on Tokio (no web framework). State is kept in JSON files; there is no database. `noise-tcp` (Noise IK over TCP) is an Experimental engine, Direct only.

## Connectivity modes

- **Direct:** either peer may initiate TCP to the other peer's reachable listener.
- **Reverse:** a peer behind NAT/firewall initiates outbound TCP to a reachable peer/hub; reverse logical streams use that established connection.
- **Relay:** a separately authorized intermediary forwards opaque end-to-end protected traffic when a direct path cannot be established.

These are connection/topology modes, not cryptographic protocols. Direct and Reverse share the same secure TCP transport adapter; only dial/listen orchestration differs. Relay must not terminate end-to-end tunnel encryption.

Path selection must use authenticated connection establishment and bounded bidirectional health checks, not ICMP alone. Auto-selection must never weaken cryptographic policy or silently downgrade transport security.

## Phase 1 data plane: TCP + TLS 1.3 mutual authentication

The Phase 1 production data plane uses TCP protected by TLS 1.3 with mutual authentication for both Direct and Reverse modes. Per-node certificate identity is bound to the enrolled logical node and explicit tunnel authorization. Authentication is not authorization. Trust-anchor constraints, rotation, revocation, and safe enrollment are required.

Use bounded Yamux logical streams over the authenticated TLS stream. Bound sessions, streams, queues, frame sizes, handshake duration, and memory. A pool of independent TCP sessions may reduce the impact of per-session head-of-line blocking but does not eliminate TCP head-of-line blocking within a session. Pool width and buffering must be measured.

TLS 1.3 0-RTT is disabled for forwarded tunnel data and non-idempotent/control operations unless a separately reviewed replay-safe design proves it safe. No plaintext or legacy TLS fallback is permitted.

## Later transport: QUIC

QUIC over UDP with TLS 1.3 and native QUIC streams is a later optional adapter. UDP availability must never be assumed; TCP remains an independent path. Do not put Yamux on top of QUIC. Direct and Reverse may use QUIC when policy and authenticated reachability permit it. Evaluate migration, congestion control, anti-amplification, resource limits, and real network compatibility before enabling it by default.

ECH (RFC 9849) is an optional TLS privacy enhancement where DNS and server deployment support it. It is not a transport and does not hide destination IP, timing, volume, or all traffic characteristics. No traffic invisibility or censorship-resistance guarantee is made.

## Identity and control plane

The proposed enrollment and issuer trust boundary is specified in [ADR-0014](ADR-0014-online-enrollment-and-control-plane.md). It remains proposed: do not expose an online enrollment endpoint or place CA signing keys in the API process until its issuer, operator-authentication, persistence, and security-review gates are closed.


Each node has a long-term logical identity and separately managed transport credentials. Enrollment tokens are single-use, short-lived, and narrowly scoped. The control plane binds the enrolled node identity to its certificate identity and supports rotation/revocation. Claimed node IDs alone are never authentication. Desired state is versioned and signed. Agents verify scope, signature, generation, and expiry; compute a deterministic plan; apply idempotently after authorization; and return a receipt. Statuses distinguish desired, planned, applying, applied, degraded, failed, and unknown. Stale telemetry is always unknown, never healthy.

## Privacy, reliability, and safety

Minimize identity and metadata in application messages, logs, metrics, and support bundles. No payload logging. Audit records describe actor, action, target, plan hash, result, and time, not forwarded traffic. Secrets stay outside logs, metrics, and support bundles.

Use bounded reconnect backoff with jitter, graceful drain, path health based on fresh measurements, and failover for new streams. A failed TCP session terminates its active logical streams unless an explicit resumable application protocol is implemented; do not claim transparent migration of established TCP streams. Sysctl/network tuning is opt-in and must restore only values changed by the product.

## Phase boundary

Phase 1 implementation is owner-approved. TCP/TLS 1.3 mTLS Direct and Reverse, node identity/authorization, key lifecycle, conformance tests, installer, and security review gates are required before production readiness. QUIC, ECH evaluation, Relay, and Noise comparison remain later work. No merge, release, or deployment is implied.
