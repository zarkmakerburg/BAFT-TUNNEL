<div align="center">

# BAFT-TUNNEL

### Secure server-to-server connectivity. Direct or Reverse.

A security-first, Linux-native tunnel platform built around authenticated connections, explicit authorization, and predictable operations.

<br />

**Private project · Rust core · TCP + TLS 1.3 · Direct / Reverse**

</div>

---

## What is BAFT-TUNNEL?

BAFT-TUNNEL connects Linux servers across networks where inbound reachability cannot be assumed. It is designed to support two connection paths:

- **Direct** — one endpoint establishes a connection to the other endpoint's reachable listener.
- **Reverse** — an endpoint behind NAT or a restrictive firewall initiates an outbound connection; logical forwarding streams travel back over that authenticated connection.

Both modes share the same security-focused transport foundation. They are connection modes, not separate cryptographic protocols.

The project is independent: no BAFT source code, Git history, credentials, release keys, or deployment state are imported.

## Project status

<sub>Updated: 7 November 2026</sub>

<blockquote>
**Active development — Phase 1**

The Rust TCP transport has passed a two-host LAB integrity test in the Direct direction, and the tcp-tls Reverse preview passed its first two-host LAB run (42506805986), including certificate-revocation checks. The Ubuntu 22.04 / systemd 249 baseline credential path (`LoadCredential=`) passes hosted CI and an isolated test on both LAB hosts (see the [implementation status](docs/PHASE1_IMPLEMENTATION_STATUS.md) for commits and runs). The project is a **pre-release and not production-ready**; long-duration soak, controlled network impairment, remote CRL distribution and CA/pin rollover remain open, and online enrollment is deferred. A separate independent security review was waived by the owner and has not been performed.
</blockquote>

| Area | Current state |
|---|---|
| Architecture and threat model | Defined |
| Rust TCP transport | Implemented; review and acceptance ongoing |
| Direct | Real two-host LAB evidence, stable engines (tcp-tls; Noise experimental) |
| Reverse (tcp-tls) | **Preview.** Real in Core and Config; two-host LAB PASS (run 42506805986: outbound-only service node, 64 KiB + 2 MiB integrity, reconnect, wrong identity/pin, revoked certificate). Breadth (R3) still open |
| Reverse (Noise) | Implemented in the core, **unproven** (no two-host LAB); not offered |
| BAFT engine | Hidden until its gates pass (ADR-0019/0020); Engine Store → Launcher execution cycle is not complete |
| Apply → Verify → Receipt | Local `setup init` / `setup join` stages through the agent and panel, gated, verified, rolled back on failure, receipt without secrets (ADR-0022). No service start, no firewall change |
| Mutual TLS and peer identity | Implemented with negative tests |
| Offline PKI and CRL revocation | Experimental; manual workflow |
| Installer and systemd unit | Implemented; Ubuntu 22.04 / systemd 249 baseline passes CI and isolated LAB test |
| Controlled SIGTERM / Ctrl-C shutdown | Implemented; bounded session close (20 seconds); active streams close on stop |
| Online enrollment / control plane | Not complete |
| Web dashboard and guided wizard | Planned |
| Production release | Not available |

**Current engineering work:** [Phase 1 implementation PR #3](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/3)

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

The two-host test exercised the current Rust runtime on the configured Iran and abroad LAB hosts:

| Test | Result |
|---|---|
| Direct forwarding | 2,097,289 bytes transferred; SHA-256 matched |
| Reverse forwarding | Same byte count and SHA-256 |
| Listener restart | Direct forwarding recovered; integrity probe passed |
| Certificate revocation | Revoked client and server certificates rejected |
| Cleanup | Temporary LAB processes and files cleaned up |

[View the real-host LAB run](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42246759289)

This is a bounded LAB result, not a production-readiness certification or a long-duration reliability claim.

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

- [Architecture and component boundaries](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/ARCHITECTURE.md)
- [Threat model](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/THREAT_MODEL.md)
- [Security guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/SECURITY_GUIDE.md)
- [Phase 1 implementation status and acceptance gaps](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/PHASE1_IMPLEMENTATION_STATUS.md)
- [Operator guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/OPERATOR_GUIDE.md)
- [Real-host LAB setup](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/phase1/rust-tcp-transport/docs/REAL_HOSTS_LAB.md)
- [Open Phase 1 pull request](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/3)
- [Engineering guardrails](docs/ENGINEERING_GUARDRAILS.md)
- [Product UX and installer acceptance](docs/PRODUCT_UX_ACCEPTANCE.md)
- [Phase 0 two-host LAB instructions](docs/PHASE0_TWO_HOST_LAB.md)
- [Phase 0 evidence history](docs/PHASE0_EXIT_EVIDENCE.md)

## Repository access and use

This is a private, owner-controlled project. No public open-source license is granted. Do not redistribute, sublicense, publish, or reuse this code without the owner's explicit written permission.

---

<div align="center">

**BAFT-TUNNEL** · Built for secure, verifiable connectivity.

<sub>Development status is documented honestly: implemented, tested, planned, and production-ready are different states.</sub>

</div>
