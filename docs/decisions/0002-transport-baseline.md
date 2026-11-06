# ADR 0002: Pooled TLS transport baseline

- Status: Accepted for Phase 0; implementation subject to Phase 1 Rust crate review
- Date: 2026-11-03

## Decision
Start with TLS 1.3 mutual-authenticated transport over a pool of independent long-lived TCP sessions. Multiplex logical streams within each session. Keep adapters replaceable.

## Trade-offs
Each individual TCP connection still experiences TCP head-of-line blocking. More sessions increase resource use and may trigger provider limits. QUIC and HTTP/2/WebSocket are later adapters, not assumed to be universally available.

## Revisit
Revisit after Phase 1 library review and Phase 4 network benchmarks provide measured evidence.
