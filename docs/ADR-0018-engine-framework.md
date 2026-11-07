# ADR-0018: Swappable engine framework and rotation

- Status: Accepted (owner approved 2026-11-07)
- Date: 2026-11-07
- Relates to: ADR-0003 (Rust core), ADR-0010 (multi-path transport)

## Context
Research shows no tunnel protocol is invisible to Iranian filtering; Reality and WireGuard are blocked. Detectability changes over time, so the product must carry several engines and switch quickly.

## Decision
1. Define a Rust `Engine` trait: `dial`, `listen`, `health`, and a stable identity/authorization contract (TLS-authenticated peer identity or Noise static key mapped to the same node ID and capability checks). Every engine keeps fail-closed authentication; no engine may weaken authorization.
2. Config gains `engine: <id>` and an ordered `engine_fallbacks` list per tunnel. A supervisor switches on health failure with capped backoff. Operator can switch manually.
3. Rollout: `tcp-tls` (exists), `noise-tcp`, `baft` (after health check of imported code), `ws-tls`, `utls-sidecar`, `quic`.
4. Camouflage needing uTLS/Reality-style fingerprints: out-of-process Go sidecar under a minimal local IPC (Unix socket, framed bytes, no secrets on argv/env). The Rust core remains the only authorizer. Go is confined to the sidecar; this narrows, not reverses, ADR-0003.
5. Imported BAFT/C-BAFT code requires: licence check, secret scan, dependency audit, tests, and conversion to the trait. Secrets are never imported.
6. Wizard exposes only engine choice, Direct/Reverse, and one port; defaults supply the rest.

## Consequences
- Refactor of the existing TCP+TLS path behind the trait must be behaviour-neutral (existing tests and LAB E2E must still pass).
- Each engine ships with LAB evidence and an honest detectability note; none is marketed as undetectable.

