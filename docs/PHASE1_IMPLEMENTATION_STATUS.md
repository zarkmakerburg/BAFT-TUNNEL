# Phase 1 implementation status

**Authorization:** Owner-approved 2026-11-06.
**Branch:** `phase1/rust-tcp-transport`.
**Status:** In progress; not production-ready. The production transport decision is ADR-0010; ADR-0009 is superseded.

## Implemented in the first transitional vertical slice

- Rust CLI with explicit `serve` and `forward` roles.
- Bounded Yamux session with explicit stream/backlog/window limits and keepalive.
- TCP forwarding from an explicitly bound local listener to one fixed remote destination.
- Bounded session and stream admission; no firewall, route, sysctl or service-manager mutation.
- Config validate/plan/explicit-confirm apply and redacted support-bundle command.
- TLS 1.3/mTLS TCP transport implemented and CI-tested; it is now the selected Phase 1 production baseline, subject to the hardening and acceptance work below.
- Initial Noise IK prototype exists in `src/noise.rs`; it is not wired into runtime and is research-only pending separate review.

## Mandatory remaining Phase 1 work

- Retain and harden TCP + TLS 1.3 mutual authentication as the single Phase 1 data-plane security adapter.
- Support both Direct and Reverse connection orchestration over the same authenticated TCP adapter.
- Bind certificate identity to enrolled node identity and explicit tunnel/stream authorization; authentication alone is insufficient.
- Implement credential issuance/enrollment, secure storage, rotation/revocation, and safe trust/pin rollover.
- Add direct and reverse two-process loopback E2E tests over the same TLS adapter.
- Add invalid/expired/revoked/untrusted certificate, wrong identity, unauthorized tunnel, replay-sensitive operation, framing, timeout, resource-limit, pool/reconnect, drain, and session-isolation tests.
- Disable TLS 1.3 early data for forwarded traffic and non-idempotent/control operations unless a reviewed replay-safe design is approved.
- Installer, systemd unit, rollback and uninstall.
- Dependency advisory/license review, protocol-boundary fuzzing, adversarial resource-exhaustion suites, and independent security review.
- Two-host real-server LAB: one Iran node and one abroad node; validate Direct/Reverse reachability, authenticated forwarding, interruption/recovery, and measured performance. No six-host topology is required.

## Later work (not Phase 1 acceptance blockers)

- QUIC adapter using native QUIC streams, with UDP-disabled fallback verification.
- ECH deployment evaluation where DNS and server infrastructure support it; document residual metadata.
- Relay topology with end-to-end encryption between tunnel endpoints.
- Noise interoperability/security comparison; do not route production traffic through the prototype without a new accepted ADR.

## Evidence and constraints

- CI evidence must be tied to the exact branch-head SHA.
- The last observed CI run on prior head `be48da90a03b91178f70645a12d1b911300e74cb` completed successfully for formatting, focused Rust tests, Clippy, and release build. This evidence does not cover subsequent documentation commits or prove the complete acceptance gate.
- Compile/test success is not real-host evidence.
- Docker/netem is simulation, not a live Iran-to-abroad path.
- Two-host real-server LAB remains NOT_RUN until the owner supplies both servers and access. These two hosts are sufficient for the required Phase 1 cross-border E2E; larger topologies are optional future scale testing.
- No production host mutation, merge, release, or deployment is authorized by this work.
