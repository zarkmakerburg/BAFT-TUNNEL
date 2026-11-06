# BAFT-TUNNEL Architecture

**Status:** Phase 0 baseline. This is a design document, not evidence that a tunnel engine exists.

## Product boundary
BAFT-TUNNEL is an independent Linux server-to-server TCP/UDP forwarding platform. Either endpoint may initiate an outbound connection; inbound reachability at both endpoints is not assumed. The product is not a subscription panel. Its API is intended for integration with third-party panels.

No BAFT implementation, history, keys, production configuration, CI secrets, or deployment assets are imported.

## Components
- **Node agent:** maintains authenticated transport sessions, listeners, forwarding sockets, health probes, and reconciliation.
- **Panel/API:** enrollment, node identity, desired state, policy, audit, and operator UI.
- **CLI:** diagnostics, enrollment, plan/apply, backup/restore, and support bundle.
- **Transport adapters:** replaceable data-plane implementations behind a versioned interface.
- **Web UI:** React + TypeScript + Vite, Persian-first RTL with English support.

The core engine, agent, API, CLI, and installer are Rust. Tokio is the async runtime, rustls provides TLS, and Axum provides HTTP/API routing. SQLite WAL is the initial store; PostgreSQL is optional.

## Initial transport baseline
Start with TLS 1.3 mutual authentication over a pool of independent long-lived TCP sessions. Multiplex logical streams inside each session. A pool reduces the blast radius of a single TCP connection's head-of-line blocking; it does not remove head-of-line blocking inside each individual TCP session. Pool width, buffering, and scheduling must be measured.

A reviewed Rust multiplexing library will be selected during Phase 1 dependency review. Its adapter must support bounded buffers, backpressure, cancellation, draining, version/capability negotiation, and conformance tests.

WebSocket/HTTP2 is a later adapter for HTTP-aware paths, not a universal CDN compatibility promise. QUIC is deferred to a later phase. UDP forwarding needs explicit framing, size limits, flow expiry, congestion control, and anti-amplification protection.

## Identity and control plane
Each node has a long-term Ed25519 identity and separate transport credentials. Enrollment tokens are single-use, short-lived, and narrowly scoped. The data plane uses TLS 1.3 mutual authentication and pinned identities. Protocol handshakes bind version, node identities, tunnel ID, capabilities, and fresh nonces.

Desired state is versioned and signed. Agents verify scope, signature, generation, and expiry; compute a deterministic plan; apply idempotently after authorization; and return a receipt. Statuses distinguish desired, planned, applying, applied, degraded, failed, and unknown. Stale telemetry is always unknown, never healthy.

## Reliability and safety
Use bounded reconnect backoff with jitter, graceful drain, path health based on fresh measurements, and failover for new streams. Do not claim transparent migration of established TCP streams without a resumable protocol. Sysctl/network tuning is opt-in and must restore only values changed by the product.

No payload logging. Audit records describe actor, action, target, plan hash, result, and time, not forwarded traffic. Secrets stay outside logs, metrics, and support bundles.

## Phase boundary
Phase 0 covers architecture, threat model, repository skeleton, and test-lab foundations only. Phase 1 implementation requires explicit owner approval.
