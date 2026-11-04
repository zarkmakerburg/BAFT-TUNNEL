# ADR 0003: Rust for the core platform

- Status: Accepted
- Date: 2026-11-04
- Scope: BAFT-TUNNEL core implementation

## Decision
Use Rust as the sole implementation language for the tunnel engine, node agent, backend/API, CLI, and installer. Use Tokio, rustls, and Axum. The web panel remains React + TypeScript + Vite. Go is not part of the core stack.

## Consequences
- Use a Cargo workspace with small, explicit crates.
- Pin dependencies and commit Cargo.lock for applications.
- Review maintenance, security advisories, license, API stability, and fuzzability before adopting protocol dependencies.
- CI tests formatting, linting, unit/integration tests, dependency audits, and Linux amd64/arm64 builds.
- This decision does not authorize Phase 1 implementation.
