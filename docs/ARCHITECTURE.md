# BAFT-TUNNEL Architecture

**Status:** Phase 1 implementation baseline; transport decision updated by ADR-0009.

## Product boundary
BAFT-TUNNEL is an independent Linux server-to-server TCP/UDP forwarding platform. Either endpoint may initiate an outbound connection; inbound reachability at both endpoints is not assumed. The product is not a subscription panel. Its API is intended for integration with third-party panels.

No BAFT implementation, history, keys, production configuration, CI secrets, or deployment assets are imported.

## Components
- **Node agent:** maintains authenticated transport sessions, listeners, forwarding sockets, health probes, and reconciliation.
- **Panel/API:** enrollment, node identity, desired state, policy, audit, and operator UI.
- **CLI:** diagnostics, enrollment, plan/apply, backup/restore, and support bundle.
- **Transport adapters:** replaceable data-plane implementations behind a versioned interface.
- **Web UI:** React + TypeScript + Vite, Persian-first RTL with English support.

The core engine, agent, API, CLI, and installer are Rust. Tokio is the async runtime; `snow` provides Noise Protocol Framework operations for the node-to-node data plane; Axum provides HTTP/API routing. rustls remains available for HTTPS/API endpoints. SQLite WAL is the initial store; PostgreSQL is optional.

## Mandatory initial data plane: reverse TCP + Noise
The node behind NAT/firewall initiates outbound TCP connections to a reachable hub. The hub accepts those connections. Reverse logical streams are then opened over the established outbound sessions; the restricted node does not need an inbound listener exposed to the public network.

Raw TCP is authenticated and encrypted with Noise, initially targeting `Noise_IK_25519_ChaChaPoly_BLAKE2s` through the reviewed `snow` crate. Each node has a pinned Noise static X25519 public key mapped to its enrolled logical identity. The initiator pins the responder key before the handshake; the responder authorizes the initiator key after the handshake. Protocol version, roles, node IDs, tunnel ID, and capabilities are cryptographically bound. No plaintext or TLS fallback is permitted.

A bounded, length-prefixed Noise framing adapter carries Yamux logical streams. Use a bounded pool of independent long-lived TCP sessions to reduce the blast radius of per-session TCP head-of-line blocking; it does not eliminate head-of-line blocking within a session. Pool width, buffering, and scheduling must be measured. Frame sizes, stream counts, connection counts, queues, and handshake duration are bounded.

The existing TLS 1.3/mTLS data-plane code is transitional and must be replaced by Noise before the Phase 1 transport acceptance gate. rustls may still be used for HTTPS control-plane endpoints.

## Other transports
WebSocket/HTTP2 is a later adapter for HTTP-aware paths, not a universal CDN compatibility promise. QUIC is deferred to a later phase. UDP forwarding needs explicit framing, size limits, flow expiry, congestion control, and anti-amplification protection.

## Identity and control plane
Each node has a long-term logical identity and a separate Noise static X25519 transport key. Enrollment tokens are single-use, short-lived, and narrowly scoped. The control plane binds the enrolled node identity to its Noise public key and supports rotation/revocation. Claimed node IDs alone are never authentication. Desired state is versioned and signed. Agents verify scope, signature, generation, and expiry; compute a deterministic plan; apply idempotently after authorization; and return a receipt. Statuses distinguish desired, planned, applying, applied, degraded, failed, and unknown. Stale telemetry is always unknown, never healthy.

## Reliability and safety
Use bounded reconnect backoff with jitter, graceful drain, path health based on fresh measurements, and failover for new streams. A failed TCP session terminates its active logical streams unless an explicit resumable application protocol is implemented; do not claim transparent migration of established TCP streams. Sysctl/network tuning is opt-in and must restore only values changed by the product.

No payload logging. Audit records describe actor, action, target, plan hash, result, and time, not forwarded traffic. Secrets stay outside logs, metrics, and support bundles.

## Phase boundary
Phase 1 implementation is owner-approved. Noise reverse-TCP transport, key lifecycle, conformance tests, and real-host validation gates are required before production readiness. No merge, release, or deployment is implied.
