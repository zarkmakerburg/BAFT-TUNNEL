# Phase 1 implementation status

**Authorization:** Owner-approved 2026-11-06.
**Branch:** `phase1/rust-tcp-transport`.
**Status:** In progress; not production-ready. Transport baseline is superseded by ADR-0009.

## Implemented in the first transitional vertical slice

- Rust CLI with explicit `serve` and `forward` roles.
- Bounded Yamux session with explicit stream/backlog/window limits and keepalive.
- TCP forwarding from an explicitly bound local listener to one fixed remote destination.
- Bounded session and stream admission; no firewall, route, sysctl or service-manager mutation.
- Config validate/plan/explicit-confirm apply and redacted support-bundle command.
- Initial TLS 1.3/mTLS transport was implemented and CI-tested, but is now transitional and is not the required final data-plane security layer.

## Mandatory remaining Phase 1 work

- Complete and integrate the initial `snow`-based Noise IK handshake/framing adapter prototype in `src/noise.rs`; it is not yet wired into the runtime.
- Replace node-to-node TLS with reverse TCP + Noise IK using pinned X25519 peer keys; run Yamux over the Noise adapter with no plaintext/TLS fallback.
- Bind enrolled node identity, tunnel ID, protocol version, role, and capabilities to the authenticated Noise session.
- Implement transport key generation/storage, peer authorization, rotation/revocation, and safe pin rollover.
- Add Noise vectors, wrong-key/prologue/identity rejection, frame-boundary, resource-limit, pool/reconnect, and two-process loopback E2E tests.
- Session-pool management and automatic recovery tests; established TCP streams are not resumed.
- Installer, systemd unit, rollback and uninstall.
- Formal interoperability, fuzzing, adversarial frame, resource-exhaustion suites.
- Six-host real-server LAB and cross-border E2E.

## Evidence and constraints

- CI must be tied to the exact branch-head SHA; transitional TLS test success does not satisfy the Noise acceptance gate.
- Compile/test success is not real-host evidence.
- Docker/netem is simulation, not a live Iran-to-abroad path.
- Real-host LAB remains NOT_RUN until the owner supplies server availability and access.
- No production host mutation, merge, release, or deployment is authorized by this work.
