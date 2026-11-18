<div align="center">

# BAFT-TUNNEL

### Secure server-to-server connectivity. Direct or Reverse.

A security-first, Linux-native tunnel platform built around authenticated connections, explicit authorization, and predictable operations.

<br />

**Public repository, all rights reserved · Rust core · TCP + TLS 1.3 · Direct / Reverse**

<a href="https://app.codspeed.io/zarkmakerburg/BAFT-TUNNEL?utm_source=badge"><img src="https://img.shields.io/endpoint?url=https://codspeed.io/badge.json" alt="CodSpeed"/></a>

</div>

---

## What is BAFT-TUNNEL?

BAFT-TUNNEL connects Linux servers across networks where inbound reachability cannot be assumed. It is designed to support two connection paths:

- **Direct** — one endpoint establishes a connection to the other endpoint's reachable listener.
- **Reverse** — an endpoint behind NAT or a restrictive firewall initiates an outbound connection; logical forwarding streams travel back over that authenticated connection.

Both modes share the same security-focused transport foundation. They are connection modes, not separate cryptographic protocols.

The project is independent: no BAFT source code, Git history, credentials, release keys, or deployment state are imported.

## Project status

<sub>Updated: 18 November 2026 · source baseline: `main` `cc411b23b851816c47476b74c03057cbb9b0f384` · roadmap: [docs/PRODUCT_ROADMAP.md](docs/PRODUCT_ROADMAP.md)</sub>

> **Stable `v0.3.0` released 18 November 2026 (signed); not production-ready.** The latest signed prerelease is `v0.3.1-rc.1`.
>
> **U7 upgrade/operations acceptance:** trusted two-host run [44368821156](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44368821156) on orchestration SHA `cc411b23b851816c47476b74c03057cbb9b0f384` passed the signed `v0.3.0` → `v0.3.1-rc.1` upgrade, one-hour soak (262 rounds, zero failed probes, zero restarts), 64 KiB/2 MiB integrity, backup/restore, guarded update and rollback, revocation refusal, uninstall and cleanup. This run did **not** exercise panel pairing or automatic pairing (`panel_pairing=0`, `auto_pair=0`).
>
> **React panel:** React is the default at `/`; the classic panel remains at `/classic` for fallback ([ADR-0054](docs/ADR-0054-react-local-agent-panel.md), [PR #350](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/350)). React `AutoPair` is implemented, but no real two-host React Pairing/PKI browser acceptance has passed. The previous D2 panel-PKI LAB pass [42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181) exercised the classic panel, not the React replacement. React Setup does not yet expose the offline CSR/certificate/CA/CRL import flow.
>
> **Fleet Add Server:** PRs [#384](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/384), [#388](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/388), [#393](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/393) and [#394](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/394) are merged. Fleet report transport passed once in [run 44338843969](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44338843969), using throwaway OpenSSL PKI rather than a `fleet_ca`-minted CA. Add Server signing is **not** end-to-end: there is no operator CA-init command and no runtime CSR-submit/join transport.
>
> **Open research:** aged-flow controls show both raw TCP and raw TLS slow sharply on connections kept alive beyond roughly 15–20 seconds ([TCP run 44101542573](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101542573), [TLS run 44101769313](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44101769313)); the cause is not attributed. S10 remains open: the latest U7 run observed an eight-line nftables address-set delta on the abroad LAB host; no BAFT-TUNNEL firewall rule was present, and the responsible process is unknown.
>
> `ready` and ordinary diagnostics `integrity` remain Unknown where evidence is absent. Firewall status remains Unknown; no component receives `CAP_NET_ADMIN`. The owner-paused independent security review remains paused. No production deployment is claimed.

| Area | Current state |
|---|---|
| Runtime | Rust, Tokio, rustls TLS 1.3, Yamux; fixed destinations and bounded sessions/streams |
| Supported baseline | Ubuntu 22.04 LTS / systemd 249 |
| Direct `tcp-tls` | Supported pre-release; real two-host evidence |
| Reverse `tcp-tls` | Preview; outbound-only data path, not Stable |
| `noise-tcp` | Experimental Direct only |
| BAFT engine | Separate pinned process; Preview Direct only; not configurable in Wizard |
| PKI | Offline CA, certificate issuance/revocation, multi-CA/CRL overlap, operator CRL renewal and SSH push |
| Agent / panel | Per-host loopback Agent; React is default at `/`, classic fallback at `/classic`. React AutoPair exists; offline CSR/certificate/CA/CRL workflow remains classic-only and React two-host Pairing/PKI acceptance is pending |
| Control plane / Fleet | Report transport LAB PASS once (run 44338843969, throwaway OpenSSL PKI). Enrollment state machine (#384), operator API (#388), Add Server UI (#393) and root signer helper (#394) are merged; `fleet_ca` initialization and runtime CSR submit/join transport are missing, so Add Server signing is not E2E-accepted |
| Release | Stable `v0.3.0` (signed); prerelease `v0.3.1-rc.1` (not Stable); Reverse, BAFT engine and stream resume remain Preview |

Project record: [HQ control board](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).

## Quick install (Ubuntu 22.04, x86_64)

One line installs the latest **signed stable** release (currently `v0.3.0`). The script is pinned to the release tag, not `main`. Before it extracts or runs anything it verifies the release's keyless Sigstore signature and SHA-256 checksums, and it stops if either check fails. It installs the panel agent only; no tunnel is started.

```bash
curl -fsSL https://raw.githubusercontent.com/zarkmakerburg/BAFT-TUNNEL/v0.3.0/scripts/quick-install.sh | sudo bash -s -- --register t1
```

`t1` is your instance name (letters, digits, `_`, `-`). To inspect the script first, download it, read it, then run it:

```bash
curl -fsSL -o quick-install.sh https://raw.githubusercontent.com/zarkmakerburg/BAFT-TUNNEL/v0.3.0/scripts/quick-install.sh
less quick-install.sh
sudo bash quick-install.sh --register t1
```

Add `--verify-only` to check the signed release without installing, `--tag vX.Y.Z` to pin another release, or `--channel prerelease` for Preview builds (Reverse and the BAFT engine are Preview). This is a CI/sandbox-verified install path; it is not a production-readiness claim.

### Automatic tunnel (pairing; no files to copy)

Available from `v0.3.0` (ADR-0050, Preview): install on both servers with the command above, then create the tunnel from the panel (Create tunnel on the first server, Connect with the code on the second), or from the shell:

```bash
# first server (accepts the connection): prints a one-time code, then asks you to approve the other server
curl -fsSL .../scripts/quick-install.sh | sudo bash -s -- --channel prerelease --offer mytunnel --service-port 8080
# second server: paste the code
curl -fsSL .../scripts/quick-install.sh | sudo bash -s -- --channel prerelease --pair 'BAFT1-...'
```

Both servers show the same short key fingerprint; approve only if they match. The code is single-use and expires in 15 minutes. Open the tunnel port (default TCP 8443) on the first server's firewall yourself; BAFT-TUNNEL never changes firewall rules. Without a terminal `--offer` refuses to run unless you add `--auto-approve`. Direct `tcp-tls` is the supported path; Reverse `tcp-tls` remains Preview and is not Stable. The panel asks whether each server is in Iran or abroad (`--location iran|abroad` on the shell); an Iran server that dials out gets a 10 s session rotation default. Two-host AutoPair passed at the Agent/API level for Direct and Reverse Preview ([44224936582](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44224936582)); that run did not exercise the React browser UI or the manual CSR/certificate/CA/CRL workflow. See [roadmap §9.10](docs/PRODUCT_ROADMAP.md) for current acceptance status.

### HTTPS panel (domain or IP)

Add `--domain` or `--ip` and the installer sets up an HTTPS front end (nginx) on the host and prints the link and the login token in a framed block at the end. Without either flag it asks on the terminal (empty answer = SSH tunnel only; `--no-https` skips the question). The panel itself stays non-root and loopback-only (ADR-0033).

```bash
# domain: Let's Encrypt certificate, auto-renewed (ports 80 and 443 must be reachable)
curl -fsSL .../scripts/quick-install.sh | sudo bash -s -- --register t1 --domain panel.example.com --email you@example.com
# IP: certificate from a CA generated on the host; the browser warns until you trust/pin the CA fingerprint that is printed
curl -fsSL .../scripts/quick-install.sh | sudo bash -s -- --register t1 --ip 203.0.113.10
```

If HTTPS setup fails it is rolled back and the panel stays loopback-only; it never falls back to plain HTTP. After installation the interactive menu opens on a terminal (`--no-menu` to skip, or run `sudo baft-tunnel menu` later). HTTPS is available from the first release that ships `scripts/panel-https.sh`.

**نصب یک‌خطی (فارسی):** دستور بالا آخرین نسخه پایدار امضاشده را نصب می‌کند. اسکریپت به تگ نسخه قفل شده (نه `main`) و پیش از هر اجرا امضای Sigstore و SHA-256 را بررسی می‌کند. برای بررسی قبل از اجرا، اسکریپت را دانلود کنید و بخوانید (دستورهای بالا)، یا با `--verify-only` فقط راستی‌آزمایی کنید. `t1` نام دلخواه نمونه است.

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

On the Ubuntu 22.04 / systemd 249 baseline, the node key source is a root-only PEM file delivered to the service with `LoadCredential=`; this is service-scoped access, not encryption at rest. `LoadCredentialEncrypted=` is an opt-in path for systemd 250+ and is not required by the baseline. Offline CA issuance, 30-day CRL renewal and operator-initiated SSH push/activation are implemented and have two-host evidence; distribution is not centrally automated. Fleet enrollment is partial: the operator offer/confirm UI/API and root signer helper are merged, but CA initialization and the node CSR-submit/join transport are not wired; no end-to-end `fleet_ca` acceptance is claimed. SSH enrollment and remote update remain deferred. A P0+P1 security assessment found no Critical or High issue; the full independent pre-stable security review is paused by the owner and has not been performed. This project does not claim to be undetectable or to provide censorship-proof connectivity.

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

The authoritative phase model and delivery order are maintained in [docs/PRODUCT_ROADMAP.md](docs/PRODUCT_ROADMAP.md). U7 signed-RC.7 MVP acceptance passed; U8 operator guides are merged. B1 is merged (`311b706`, `62d7ee6`). B2 (Direct integrity probe) is accepted; B3–B6 follow in roadmap order. Later Fleet/control-plane and additional transport work remains gated; do not infer that a planned item is shipped from this README.

## Repository guide

- [Architecture and component boundaries](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/ARCHITECTURE.md)
- [Threat model](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/THREAT_MODEL.md)
- [Security guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/SECURITY_GUIDE.md)
- [Phase 1 implementation status and acceptance gaps](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/PHASE1_IMPLEMENTATION_STATUS.md)
- [Operator guide](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/OPERATOR_GUIDE.md) (interactive install menu: `sudo scripts/install.sh` or `sudo baft-tunnel menu`, Persian/English)
- [Real-host LAB setup](https://github.com/zarkmakerburg/BAFT-TUNNEL/blob/main/docs/REAL_HOSTS_LAB.md)
- [Connection topology and Reverse (ADR-0021)](docs/ADR-0021-connection-topology-and-reverse.md)
- [React local Agent panel (ADR-0054)](docs/ADR-0054-react-local-agent-panel.md)
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
