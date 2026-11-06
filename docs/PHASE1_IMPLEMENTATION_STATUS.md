# Phase 1 implementation status

**Authorization:** Owner-approved 2026-11-06.
**Branch:** `phase1/rust-tcp-transport`.
**Status:** In progress; not production-ready. The production transport decision is ADR-0010; ADR-0009 is superseded.

## Implemented in the first transitional vertical slice

- Rust CLI with explicit `serve` and `forward` roles.
- Bounded Yamux session with keepalive, an explicit 4 MiB per-stream receive window, at most 8 streams per session, and at most 8 concurrent sessions (aggregate theoretical receive-window budget: 256 MiB).
- TCP forwarding from an explicitly bound local listener to one fixed remote destination.
- Bounded session and stream admission; no firewall, route, sysctl or service-manager mutation.
- Config validate/plan/explicit-confirm apply and redacted support-bundle command.
- TLS 1.3/mTLS TCP transport implemented; the peer leaf certificate must contain exactly one BAFT-TUNNEL node URI SAN matching the enrolled logical node (urn:baft-tunnel:node:<node-id>), parsed with the vetted x509-parser crate. Missing, duplicate, conflicting, malformed, or trailing certificate data is rejected. Certificate-chain validation and SHA-256 pinning remain in force.
- TLS 1.3 early data is explicitly disabled in both client and server configurations. The TLS integration test checks correct and incorrect node identity on both sides.
- Initial Noise IK prototype exists in `src/noise.rs`; it is not wired into runtime and is research-only pending separate review.
- Phase 0 two-host real-server preflight completed successfully on 2026-11-06 (GitHub Actions run [42221903167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42221903167)). Both configured hosts accepted strict-host-key SSH inspection; both reported Ubuntu 22.04.x, x86_64, systemd, and the required baseline tools. The Iran-to-abroad TCP probes completed; all four tested ports reported reachable host/no listener.

## Mandatory remaining Phase 1 work

- Retain and harden TCP + TLS 1.3 mutual authentication as the single Phase 1 data-plane security adapter.
- Support both Direct and Reverse connection orchestration over the same authenticated TCP adapter.
- Enforce explicit authorization of the authenticated node for each tunnel, connection direction, and stream capability; current fixed peer/tunnel checks are not yet a complete policy model.
- Implement credential issuance/enrollment, secure storage, rotation/revocation, and safe trust/pin rollover.
- Two-process loopback E2E coverage exercises authenticated TCP/TLS/Yamux forwarding with each side acting as the dialer; both orientations pass with a 2 MiB + 137-byte payload, validating the configured Yamux flow-control window.
- The manual GitHub Actions workflow provisions short-lived certificates and temporary binaries under /tmp on both LAB hosts, tests Direct and Reverse forwarding, verifies payload integrity and listener interruption/recovery, then cleans up. It does not change firewall, routes, sysctl, or service-manager state. Real-host run [42236737480](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42236737480) passed on 2026-11-06.
- Add invalid/expired/revoked/untrusted certificate rejection tests.
- Add unauthorized tunnel/direction, replay-sensitive operation, framing, timeout, resource-limit, reconnect, drain, and session-isolation integration tests.
- Installer, systemd unit, rollback and uninstall.
- Dependency advisory/license review, protocol-boundary fuzzing, adversarial resource-exhaustion suites, and independent security review.
- Extend LAB performance and resilience evidence with longer-duration runs and controlled loss/latency profiles; the current successful run covers direct/reverse forwarding, 2 MiB integrity, and listener restart/recovery on the two configured hosts.

## Later work (not Phase 1 acceptance blockers)

- QUIC adapter using native QUIC streams, with UDP-disabled fallback verification.
- ECH deployment evaluation where DNS and server infrastructure support it; document residual metadata.
- Relay topology with end-to-end encryption between tunnel endpoints.
- Noise interoperability/security comparison; do not route production traffic through the prototype without a new accepted ADR.

## Evidence and constraints

- Current exact branch head: `01285d83fb6cfcc3f2c474072cf82317dfbbb887`.
- Exact-head `phase1-rust` CI run [42236683776](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42236683776) passed: formatting, 19 Rust tests, Clippy with warnings denied, and release build.
- Exact-head Phase 0 checks run [42236683874](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42236683874) passed, including shell checks, workflow pin checks, mock SSH/SCP orchestration tests, and isolated Docker/netem integration.
- Two-host real-host LAB run [42236737480](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42236737480) passed. Direct (Iran dials abroad): 2,097,289 bytes, SHA-256 `1bb59406b95f296d1ffac9536e3c14a5dfc67c2632ca9ae26d1c85ca8b414fa8`, 0.869 s. Reverse (abroad dials Iran): same byte count and SHA-256, 0.749 s. The 64 KiB probes also passed in both directions. After restarting the abroad listener, Direct forwarding recovered and passed a further 64 KiB integrity probe in 1.481 s.
- The real-host workflow used only temporary binaries, certificates, and processes under `/tmp`; the workflow cleanup step passed. No firewall, route, sysctl, systemd, or production configuration was changed.
- Docker/netem remains simulation, not a substitute for the live Iran-to-abroad evidence above.
- No production host mutation or deployment has been performed. PR #3 remains Draft because explicit per-tunnel/direction/stream authorization, credential lifecycle, additional negative-security tests, installer/rollback, dependency/fuzz/security review, and other acceptance gates remain open.
