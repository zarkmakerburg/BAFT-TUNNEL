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
- Phase 0 two-host real-server preflight completed successfully on 2026-11-06 (GitHub Actions run [42221903167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42221903167)). Both configured hosts accepted strict-host-key SSH inspection; both reported Ubuntu 22.04.x, x86_64, systemd, and the required baseline tools. The Iran-to-abroad TCP probes completed; all four tested ports reported reachable host/no listener.

## Mandatory remaining Phase 1 work

- Retain and harden TCP + TLS 1.3 mutual authentication as the single Phase 1 data-plane security adapter.
- Support both Direct and Reverse connection orchestration over the same authenticated TCP adapter.
- Bind the validated certificate SAN identity to the enrolled logical node identity and enforce explicit tunnel/stream authorization. The current application handshake exchanges claimed node IDs; those claims must not be treated as certificate identity.
- Implement credential issuance/enrollment, secure storage, rotation/revocation, and safe trust/pin rollover.
- Add direct and reverse two-process loopback E2E tests over the same TLS adapter.
- Add invalid/expired/revoked/untrusted certificate, wrong certificate identity, unauthorized tunnel, replay-sensitive operation, framing, timeout, resource-limit, pool/reconnect, drain, and session-isolation tests.
- Disable TLS 1.3 early data for forwarded traffic and non-idempotent/control operations unless a reviewed replay-safe design is approved.
- Installer, systemd unit, rollback and uninstall.
- Dependency advisory/license review, protocol-boundary fuzzing, adversarial resource-exhaustion suites, and independent security review.
- Two-host Phase 1 real-server LAB: validate actual Direct/Reverse tunnel establishment, authenticated forwarding, interruption/recovery, and measured performance. Phase 0 host inspection and TCP reachability are not Phase 1 tunnel E2E evidence.

## Later work (not Phase 1 acceptance blockers)

- QUIC adapter using native QUIC streams, with UDP-disabled fallback verification.
- ECH deployment evaluation where DNS and server infrastructure support it; document residual metadata.
- Relay topology with end-to-end encryption between tunnel endpoints.
- Noise interoperability/security comparison; do not route production traffic through the prototype without a new accepted ADR.

## Evidence and constraints

- CI evidence must be tied to the exact branch-head SHA.
- The last observed CI run on branch head `66bf647a9d45e929306c1ca4845ad2bcb0a2f7a4` completed successfully for the `phase1-rust` workflow (formatting, focused Rust tests, Clippy, and release build); run [42212748324](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42212748324). This does not prove the complete acceptance gate.
- The Phase 0 real-host preflight is successful, but it is limited to host facts and TCP connection probes. It does not prove tunnel functionality, throughput, resilience, censorship resistance, or production readiness.
- Docker/netem is simulation, not a live Iran-to-abroad tunnel path.
- Phase 1 real-host tunnel E2E remains NOT_RUN.
- No production host mutation, merge, release, or deployment is authorized by this work.
