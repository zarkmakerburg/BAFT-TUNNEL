# Phase 1 implementation status

**Authorization:** Owner-approved 2026-11-06.
**Branch:** `phase1/rust-tcp-transport`.
**Status:** In progress; not production-ready.

## Implemented in the first vertical-slice commits

- Rust CLI with explicit `serve` and `forward` roles.
- TLS 1.3 only, mutual certificate authentication, private CA trust, and configured SHA-256 leaf-certificate pin.
- Versioned bounded handshake binding local/peer identity, tunnel ID and fresh per-connection nonces.
- Tokio-native Yamux session with explicit stream/backlog/window limits and keepalive.
- TCP forwarding from an explicitly bound local listener to one fixed remote destination.
- Bounded session and stream admission; no firewall, route, sysctl or service-manager mutation.
- Focused unit tests for identity/fingerprint validation and handshake identity binding.

## Not implemented yet

- Session-pool management and automatic recovery tests; the source-side forwarder now retries with bounded exponential backoff and jitter, but established TCP streams are not resumed.
- Identity issuance, rotation, revocation and enrollment.
- CLI plan/apply and support-bundle commands.
- Installer, systemd unit, rollback and uninstall.
- Formal interoperability, fuzzing, adversarial frame, resource-exhaustion and loopback end-to-end suites.
- Six-host real-server LAB and cross-border E2E.

## Evidence

- CI must be tied to the exact branch-head SHA.
- Compile/test success is not real-host evidence.
- Docker/netem is simulation, not a live Iran-to-abroad path.
- No server credentials are needed for this implementation stage; real-host operations remain NOT RUN.
- PR #1 (Phase 0) remains a separate Draft and is not merged by this work.
- No release, deployment or production mutation is authorized by the Phase 1 start.
