<div align="center">

# BAFT-TUNNEL

### Secure server-to-server connectivity. Direct or Reverse.

A security-first, Linux-native tunnel platform built around authenticated connections, explicit authorization, and predictable operations.

<br />

**Public repository, all rights reserved · Rust core · TCP + TLS 1.3 · Direct / Reverse**

</div>

---

## What is BAFT-TUNNEL?

BAFT-TUNNEL connects Linux servers across networks where inbound reachability cannot be assumed. It is designed to support two connection paths:

- **Direct** — one endpoint establishes a connection to the other endpoint's reachable listener.
- **Reverse** — an endpoint behind NAT or a restrictive firewall initiates an outbound connection; logical forwarding streams travel back over that authenticated connection.

Both modes share the same security-focused transport foundation. They are connection modes, not separate cryptographic protocols.

The project is independent: no BAFT source code, Git history, credentials, release keys, or deployment state are imported.

## Project status

<sub>Updated: 8 November 2026 · evidence tied to `main` at `2d8cd5e`</sub>

<blockquote>
**Active development — Phase 1, pre-release, not production-ready**

- **Direct** is the supported mode. It passed the two-host LAB (Iran and abroad hosts, Ubuntu 22.04 / systemd 249) with 64 KiB and 2 MiB integrity checks, listener-restart recovery and certificate-revocation checks.
- **Reverse** for `tcp-tls` is **implemented in the Rust core and configuration** (the service node dials out; the public node accepts and ingresses). It passed a dedicated two-host LAB run (see below). It is a **preview**: it is **not Stable**, the panel, wizard and capability API offer it with a Preview label, and `noise-tcp` Reverse is implemented but unproven and not offered.
- The **BAFT engine** (pinned Go binary, separate process) passed a two-host LAB run under hardened systemd 249 units. It is still hidden from the wizard; the Engine Store is not yet read by the Launcher.
- Still open: multi-hour soak, real packet-loss test, remote CRL distribution and CA/pin rollover, online enrollment, service start and end-to-end verification through Apply (Apply, Verify, Receipt covers only the local `setup init`/`join` stages, ADR-0022). An independent security review was waived by the owner and has **not** been performed.
</blockquote>

| Area | Current state |
|---|---|
| Architecture and threat model | Defined |
| Rust TCP transport (`tcp-tls`) | Implemented; review and acceptance ongoing |
| Direct | Real two-host LAB evidence, stable engines (tcp-tls; Noise experimental) |
| Reverse (tcp-tls) | **Preview.** Real in Core and Config; two-host LAB PASS (run 42506805986: outbound-only service node, 64 KiB + 2 MiB integrity, reconnect, wrong identity/pin, revoked certificate). Breadth (R3) still open |
| Reverse (Noise) | Implemented in the core, **unproven** (no two-host LAB); not offered |
| BAFT engine | Hidden until its gates pass (ADR-0019/0020); Engine Store → Launcher execution cycle is not complete |
| Apply → Verify → Receipt | Local `setup init` / `setup join` stages through the agent and panel, gated, verified, rolled back on failure, receipt without secrets (ADR-0022). No service start, no firewall change |
| Mutual TLS and peer identity | Implemented with negative tests |
| Offline PKI and CRL revocation | Experimental; manual workflow |
| Installer and systemd unit | Implemented; Ubuntu 22.04 / systemd 249 baseline passes CI and isolated LAB test |
| BAFT engine host | Pinned engine built and shipped in signed releases; LAB PASS under systemd 249; Engine Store to Launcher wiring open; hidden from the wizard |
| Web panel and guided wizard | Read-only preview; Apply, Verify and Receipt not implemented |
| Online enrollment / control plane | Not complete |
| Production release | Not available |

Project record: [HQ control board](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).

## Security model

Security is part of the transport design, not an optional add-on.

- TLS 1.3 mutual authentication for the current TCP data plane.
- Explicit peer identity, tunnel identity, direction, and capability authorization.
- No plaintext fallback or silent downgrade.
- Revocation-aware certificate validation using CA-signed CRLs.
- Bounded frames, sessions, streams, queues, and connection deadlines.
- Fixed forwarding destinations; a remote peer cannot select arbitrary targets.
- No forwarded-payload logging.
- Least-privilege service execution and hardened systemd defaults.

The current node private keys are unencrypted PEM files. Offline CA issuance and CRL distribution are manual. Encrypted key custody, online enrollment, automated revocation distribution, key/pin rotation, and further adversarial testing remain open work; independent security review was waived by the owner and has not been performed. This project does not claim to be undetectable or to provide censorship-proof connectivity.

## Technology

| Layer | Technology |
|---|---|
| Core, agent, API, CLI | Rust |
| Async runtime | Tokio |
| Secure transport | rustls · TLS 1.3 |
| Multiplexing | Yamux |
| HTTP/API | Axum |
| Web interface | React · TypeScript · Vite |
| Initial persistence | SQLite WAL |

QUIC is a planned optional transport, not part of the current production data plane. The isolated Noise IK prototype is research-only and is not used to carry tunnel traffic.

## Verified LAB evidence

All runs used the configured Iran and abroad LAB hosts with temporary files and processes only; cleanup passed. Each result is tied to one commit and one workflow run.

| Run | Commit | Result |
|---|---|---|
| [42506805986](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42506805986) (Reverse, `tcp-tls`) | `2d8cd5e` | PASS: outbound-only service node, 64 KiB and 2 MiB integrity (SHA-256 matched), reconnect after restarting either node, busy port refused, wrong identity refused, wrong pin refused, revoked certificate refused, recovery after refusals; Direct and revocation checks also passed |
| [42504156305](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42504156305) (BAFT engine) | `2d8cd5e` | PASS: Direct and BAFT's own reverse path with 2 MiB integrity, unauthorized identity, wrong pin, revoked client and revoked listener refused, bad binary hash refused to start, hardened systemd 249 units (`NoNewPrivileges`, `MemoryDenyWriteExecute`), listener-unit recovery and engine SIGKILL recovery |

Earlier "Reverse" LAB results (before 8 November 2026) tested a label only, not a reverse data path, and are **not** evidence for the current Reverse implementation (see [ADR-0021](docs/ADR-0021-connection-topology-and-reverse.md)).

These are bounded LAB results, not a production-readiness certification or a long-duration reliability claim. Soak, real packet loss and `noise-tcp` Reverse are untested.

## Engineering principles

1. **Verify before declaring healthy.** Missing or stale telemetry is unknown, never healthy.
2. **Plan before applying.** Mutations should be previewed, authorized, recorded, and recoverable.
3. **Authenticate, then authorize.** A valid certificate alone does not grant tunnel access.
4. **Fail closed.** Invalid identity, policy, revocation data, or protocol framing must not silently weaken security.
5. **Keep operations understandable.** Safe defaults and guided workflows should make routine setup approachable without hiding risk.
6. **Prove the environment.** Simulation, mock tests, and real-host LAB results are reported separately.

## Roadmap

| Stage | Focus |
|---|---|
| Phase 0 | Architecture, threat model, repository and LAB foundation |
| **Phase 1 — active** | Rust transport, identity, authorization, CLI, PKI, installer, security testing |
| Phase 2 | Enrollment, control plane, agent reconciliation, signed desired state |
| Phase 3 | Web dashboard, setup wizard, monitoring and alerts |
| Phase 4 | Optional QUIC, path selection, failover and additional transports |
| Phase 5 | Hardening, independent security review, operational readiness and release |

A phase is complete only when its acceptance evidence is reviewed. A roadmap item is not a shipped feature.

## Repository guide

- [Architecture and component boundaries](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/ARCHITECTURE.md)
- [Threat model](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/THREAT_MODEL.md)
- [Security guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/SECURITY_GUIDE.md)
- [Phase 1 implementation status and acceptance gaps](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/PHASE1_IMPLEMENTATION_STATUS.md)
- [Operator guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/OPERATOR_GUIDE.md)
- [Real-host LAB setup](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/REAL_HOSTS_LAB.md)
- [Connection topology and Reverse (ADR-0021)](docs/ADR-0021-connection-topology-and-reverse.md)
- [Product roadmap](docs/PRODUCT_ROADMAP.md)
- [HQ control board](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2)
- [Engineering guardrails](docs/ENGINEERING_GUARDRAILS.md)
- [Product UX and installer acceptance](docs/PRODUCT_UX_ACCEPTANCE.md)
- [Phase 0 two-host LAB instructions](docs/PHASE0_TWO_HOST_LAB.md)
- [Phase 0 evidence history](docs/PHASE0_EXIT_EVIDENCE.md)

## Repository access and use

The source is publicly viewable on GitHub, but this is an owner-controlled project and **no open-source license is granted**: all rights are reserved. Do not redistribute, sublicense, or reuse this code without the owner's explicit written permission.

---

<div align="center">

**BAFT-TUNNEL** · Built for secure, verifiable connectivity.

<sub>Development status is documented honestly: implemented, tested, planned, and production-ready are different states.</sub>

</div>
