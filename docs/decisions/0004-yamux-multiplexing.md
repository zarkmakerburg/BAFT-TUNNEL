# ADR 0004: Tokio-native Yamux multiplexing

- **Status:** Accepted for the Phase 1 vertical slice; subject to exact-SHA CI and dependency/security review before merge.
- **Date:** 2026-11-06
- **Scope:** One authenticated TLS 1.3 session carrying bounded logical TCP streams.

## Decision

Use **`tokio-yamux = 0.3.20`** behind the transport boundary. Docs.rs identifies it as a Rust implementation of HashiCorp Yamux; the 0.3.20 release is listed as published 2026-08-17. The published crate manifest for the 0.3.19 line declares MIT licensing, Rust 1.85, Tokio 1.x and tokio-util codec integration; the resolved 0.3.20 package metadata and checksum must be captured in Cargo.lock before merge. Its API exposes bounded stream count, accept backlog, stream window, keepalive and write timeout.

## Alternatives

| Candidate | Reason not selected |
|---|---|
| `yamux` 0.14.x | Runtime-neutral futures I/O API; adds adapter friction to the Tokio-first baseline. |
| `smux` / `async_smux` | Insufficient independent compatibility/security evidence for this first security-sensitive slice. |
| Custom framing | Rejected; duplicates parser, flow-control and lifecycle attack surface. |

## Constraints

- Yamux runs over reliable ordered streams and does not remove TCP head-of-line blocking within a carrier connection.
- Initial settings: max 64 streams, accept backlog 64, 1 MiB stream-window cap, 15s keepalive and 10s write timeout.
- A process-wide semaphore separately limits active forwarded streams; accepted TLS sessions are capped at 32.
- Keepalive alone is not a Ready verdict. Readiness requires evidence from actual forwarded traffic.
- No UDP/QUIC or cross-language compatibility claim is made in Phase 1.

## Required verification before merge

1. Commit Cargo.lock with exact versions and checksums; review the full dependency graph and license metadata.
2. Run formatting, tests, Clippy and release build on the exact commit.
3. Add loopback tests for bidirectional byte integrity, concurrent stream isolation, stream-cap enforcement, reset and session shutdown.
4. Review malformed-frame and resource-exhaustion behavior. Upstream tests do not replace BAFT-TUNNEL integration tests.
5. Check dependency advisories and licensing against the resolved lockfile.

Reopen this ADR if limits, backpressure or lifecycle behavior cannot be demonstrated.
