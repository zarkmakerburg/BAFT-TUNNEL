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

<sub>Updated: 11 November 2026 · source baseline: `main` `52a2825` · roadmap: [docs/PRODUCT_ROADMAP.md](docs/PRODUCT_ROADMAP.md)</sub>

> **Active development — signed pre-release `v0.1.0-rc.7`; not production-ready.** Trusted U7 acceptance passed on the signed RC.7 package in [run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428): both LAB hosts completed setup, Direct traffic integrity, 3600-second soak, backup/restore, update/rollback, revocation and uninstall. U8 operator documentation was completed and merged in [PR #165](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/165) (merge `52a2825254f8254386f71af7220c7cb244200fc2`). B1 structured localized evidence is in [PR #168](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/168); the latest exact head `bd784300fcc2928cc6412363d423da6542451d33` has phase0, fuzz-smoke, same-SHA package/signature verification, and the Phase 1 panel-ui, Ubuntu 22.04/systemd 249, and offline-CA jobs passing; the Rust job is still running, so exact-head CI is not complete and merge remains blocked.

- **`tcp-tls` Direct:** supported pre-release path with real two-host integrity, recovery, revocation, operations, and bounded soak/loss evidence.
- **`tcp-tls` Reverse:** real outbound-only data path with recovery and impairment evidence; remains **Preview**, not Stable.
- **`noise-tcp`:** Experimental Direct only. Reverse is not accepted or offered.
- **BAFT engine:** separate, pinned Go process; **Preview, Direct only**. It is visible in the catalogue but guided setup cannot yet configure it.
- **Panel PKI (D2):** panel → local Agent → root helper → isolated offline CA → certificate/CA/CRL import → pairing → Start passed the trusted two-host repeat ([run 42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181)). This is Direct evidence and does not imply Stable readiness.
- **CRL renewal/push (U1):** a 30-day CRL was renewed offline, pushed and activated on both LAB nodes, and tunnel recovery/integrity passed ([run 42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521), product-code SHA `8be7437`, an ancestor of current main).
- Apply receipts report observed effects; `ready` remains **Unknown** by design. The panel and Agent are local to each host; there is no central Fleet or online enrollment service.
- U7 scope is Direct only; `ready` and ordinary diagnostics `integrity` remain Unknown. An unrelated six-line firewall/address-set drift on the abroad LAB host remains open as S10; no BAFT-TUNNEL firewall rules were present. Stable release and production deployment have not occurred. The owner paused the independent pre-stable security review; it must not be restarted without a new instruction. LAB soak is capped at one hour.

| Area | Current state |
|---|---|
| Runtime | Rust, Tokio, rustls TLS 1.3, Yamux; fixed destinations and bounded sessions/streams |
| Supported baseline | Ubuntu 22.04 LTS / systemd 249 |
| Direct `tcp-tls` | Supported pre-release; real two-host evidence |
| Reverse `tcp-tls` | Preview; outbound-only data path, not Stable |
| `noise-tcp` | Experimental Direct only |
| BAFT engine | Separate pinned process; Preview Direct only; not configurable in Wizard |
| PKI | Offline CA, certificate issuance/revocation, multi-CA/CRL overlap, operator CRL renewal and SSH push |
| Agent / panel | Per-host loopback Agent and dependency-free static JavaScript panel; Persian RTL and English LTR |
| Control plane / Fleet | Not implemented; online enrollment is deferred |
| Release | Signed prereleases only; no stable release |

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

On the Ubuntu 22.04 / systemd 249 baseline, the node key source is a root-only PEM file delivered to the service with `LoadCredential=`; this is service-scoped access, not encryption at rest. `LoadCredentialEncrypted=` is an opt-in path for systemd 250+ and is not required by the baseline. Offline CA issuance, 30-day CRL renewal and operator-initiated SSH push/activation are implemented and have two-host evidence; distribution is not centrally automated. Online enrollment and central control-plane work remain deferred. A P0+P1 security assessment found no Critical or High issue; the full independent pre-stable review is paused by the owner and has not been performed. This project does not claim to be undetectable or to provide censorship-proof connectivity.

## Technology

| Layer | Technology |
|---|---|
| Core, agent, API, CLI | Rust |
| Async runtime | Tokio |
| Secure transport | rustls · TLS 1.3 |
| Multiplexing | Yamux |
| Alternative engine | Noise IK over TCP (`noise-tcp`, Experimental) |
| HTTP/API | Small loopback HTTP server on Tokio (no framework) |
| Web interface | Plain JavaScript, no build step; Persian-first RTL with English |
| Persistence | JSON files in the state directory |

QUIC is a planned optional transport, not part of the current data plane.

## Verified LAB evidence

All runs used the configured Iran and abroad LAB hosts with temporary files and processes only; cleanup passed. Each result is tied to one commit and one workflow run.

| Run | Commit | Result |
|---|---|---|
| [42506805986](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42506805986) (Reverse, `tcp-tls`) | `2d8cd5e` | PASS: outbound-only service node, 64 KiB and 2 MiB integrity (SHA-256 matched), reconnect after restarting either node, busy port refused, wrong identity refused, wrong pin refused, revoked certificate refused, recovery after refusals; Direct and revocation checks also passed |
| [42504156305](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42504156305) (BAFT engine) | `2d8cd5e` | PASS: Direct and BAFT's own reverse path with 2 MiB integrity, unauthorized identity, wrong pin, revoked client and revoked listener refused, bad binary hash refused to start, hardened systemd 249 units (`NoNewPrivileges`, `MemoryDenyWriteExecute`), listener-unit recovery and engine SIGKILL recovery |
| [42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181) (trusted D2 PKI repeat) | immutable signed RC.7 candidate | PASS: installed Agent + panel, offline CA ceremony, CSR/certificate/CA/CRL import, pairing, Start and tunnel checks; `host_drift=none` |
| [42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521) (U1 CRL renewal/push) | `8be7437` (ancestor of current main) | PASS: offline 30-day CRL renewal, SSH push and activation on both nodes, CRL number `0x1002→0x1003`, 2 MiB recovery/integrity, short and replayed CRLs refused |
| [43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) (trusted U7 integrated MVP acceptance) | `1219f547` | **PASS:** release-tag, signed-asset verification and real two-host acceptance all completed successfully. See the U7 acceptance record in the roadmap and HQ Issue #2. |

Earlier "Reverse" LAB results (before 8 November 2026) tested a label only, not a reverse data path, and are **not** evidence for the current Reverse implementation (see [ADR-0021](docs/ADR-0021-connection-topology-and-reverse.md)).

These are bounded LAB results, not a production-readiness certification or a long-duration reliability claim. The project does not require a multi-hour soak; any soak is capped at 60 minutes. `noise-tcp` Reverse is untested and unavailable.

## Engineering principles

1. **Verify before declaring healthy.** Missing or stale telemetry is unknown, never healthy.
2. **Plan before applying.** Mutations should be previewed, authorized, recorded, and recoverable.
3. **Authenticate, then authorize.** A valid certificate alone does not grant tunnel access.
4. **Fail closed.** Invalid identity, policy, revocation data, or protocol framing must not silently weaken security.
5. **Keep operations understandable.** Safe defaults and guided workflows should make routine setup approachable without hiding risk.
6. **Prove the environment.** Simulation, mock tests, and real-host LAB results are reported separately.

## Roadmap

The authoritative phase model and delivery order are maintained in [docs/PRODUCT_ROADMAP.md](docs/PRODUCT_ROADMAP.md). U7 signed-RC.7 MVP acceptance passed; U8 operator guides are merged. B1 structured localized evidence is implemented in PR #168; exact-head CI on `bd784300...` is still running (Rust job). After acceptance and merge, B2–B6 follow in roadmap order. Later Fleet/control-plane and additional transport work remains gated; do not infer that a planned item is shipped from this README.

## Repository guide

- [Architecture and component boundaries](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/ARCHITECTURE.md)
- [Threat model](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/THREAT_MODEL.md)
- [Security guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/SECURITY_GUIDE.md)
- [Phase 1 implementation status and acceptance gaps](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/PHASE1_IMPLEMENTATION_STATUS.md)
- [Operator guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/OPERATOR_GUIDE.md) (interactive install menu: `sudo scripts/install.sh` or `sudo baft-tunnel menu`, Persian/English)
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
