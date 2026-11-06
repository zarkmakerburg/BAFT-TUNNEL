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
- TLS 1.3/mTLS TCP transport implemented; the peer leaf certificate is bound to the enrolled logical node using the exact URI SAN contract urn:baft-tunnel:node:<node-id> with the vetted x509-parser crate. Certificate-chain validation and SHA-256 pinning remain in force.
- TLS 1.3 early data is explicitly disabled in both client and server configurations.
- Initial Noise IK prototype exists in `src/noise.rs`; it is not wired into runtime and is research-only pending separate review.
- Phase 0 two-host real-server preflight completed successfully on 2026-11-06 (GitHub Actions run [42221903167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42221903167)). Both configured hosts accepted strict-host-key SSH inspection; both reported Ubuntu 22.04.x, x86_64, systemd, and the required baseline tools. The Iran-to-abroad TCP probes completed; all four tested ports reported reachable host/no listener.

## Mandatory remaining Phase 1 work

- Retain and harden TCP + TLS 1.3 mutual authentication as the single Phase 1 data-plane security adapter.
- Support both Direct and Reverse connection orchestration over the same authenticated TCP adapter.
- Enforce explicit authorization of the authenticated node for each tunnel, connection direction, and stream capability; current fixed peer/tunnel checks are not yet a complete policy model.
- Implement credential issuance/enrollment, secure storage, rotation/revocation, and safe trust/pin rollover.
- Add direct and reverse two-process loopback E2E tests over the same TLS adapter.
- Add invalid/expired/revoked/untrusted certificate and wrong-identity rejection tests.
- Add unauthorized tunnel/direction, replay-sensitive operation, framing, timeout, resource-limit, reconnect, drain, and session-isolation integration tests.
- Installer, systemd unit, rollback and uninstall.
- Dependency advisory/license review, protocol-boundary fuzzing, adversarial resource-exhaustion suites, and independent security review.
- Two-host Phase 1 real-server LAB: validate actual Direct/Reverse tunnel establishment, authenticated forwarding, interruption/recovery, and measured performance. Phase 0 host inspection and TCP reachability are not Phase 1 tunnel E2E evidence.

## Later work (not Phase 1 acceptance blockers)

- QUIC adapter using native QUIC streams, with UDP-disabled fallback verification.
- ECH deployment evaluation where DNS and server infrastructure support it; document residual metadata.
- Relay topology with end-to-end encryption between tunnel endpoints.
- Noise interoperability/security comparison; do not route production traffic through the prototype without a new accepted ADR.

## Evidence and constraints

- CI evidence is tied to exact branch-head SHA `99456e16cb2ceb9a1da11a9761a2a635e06d2e25`.
- The `phase1-rust` workflow completed successfully on this exact head in run [42228819817](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42228819817): formatting, all-target Rust tests, Clippy, and release build all passed.
- The Phase 0 real-host preflight is successful, but it is limited to host facts and TCP connection probes. It does not prove tunnel functionality, throughput, resilience, censorship resistance, or production readiness.
- Docker/netem is simulation, not a live Iran-to-abroad tunnel path.
- Phase 1 real-host tunnel E2E remains NOT_RUN.
- No production host mutation or deployment has been performed. Merge, release, and deployment remain gated on the complete Phase 1 acceptance criteria and real-host LAB evidence.
