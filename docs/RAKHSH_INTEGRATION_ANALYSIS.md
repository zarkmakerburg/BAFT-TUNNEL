# Rakhsh integration analysis

Status: living document, opened 2026-11-17. Related: ADR-0038 (licence boundary), ADR-0039 (carrier link policy), ADR-0040 (stream resume), ADR-0041 (transport health), ADR-0019.

Evidence scope: **read-only source review** of `CluvexStudio/rakhsh` at `89fa8e131f5d423fb709a9c9c3c28cbc0cee39cd` (2026-11-12) against BAFT-TUNNEL main `69f43ed` (2026-11-17). Nothing here is LAB, VM or production evidence. No Rakhsh code was executed.

## 0. Decision that shapes everything

Update 2026-11-17: owner answered B-1 ("copy code if needed"). ADR-0043 allows copying only into an isolated AGPL unit, never into the proprietary core. Evaluation result: nothing is both useful and isolable, so nothing is copied today. The text below is the original analysis and is still the reason the in-process features are rewritten.

Rakhsh is **AGPL-3.0-only** (`LICENSE`, `Cargo.toml`). The BAFT-TUNNEL Rust core is **proprietary, all rights reserved** (`Cargo.toml` `LicenseRef-Proprietary`, README licence section). Copying or adapting Rakhsh source into the core would make the core a derivative work under AGPL-3.0 (copyleft, network-use source offer), which contradicts ADR-0019 clause 5 ("Rust stays proprietary and never links AGPL code").

**Result: no Rakhsh code is transferred.** Every port below is "independent implementation from ideas and behaviour documented by Rakhsh's user docs", under the clean-room rules in ADR-0038. Whether to relicense BAFT-TUNNEL under AGPL is an owner decision (blocker B-1), not an engineering one, and is not assumed here.

## 1. Rakhsh architecture (from source tree and docs)

Single Rust binary `rakhsh` plus a separate `rakhsh-control` panel/bot (workspace member `control/`). The wire protocol lives in a **separate repository** (`shabdiz`, a `path = "../shabdiz"` dependency, not reviewed, licence unknown).

| Area | Files (size) | Notes |
|---|---|---|
| Carriers / links | `src/carrier.rs` (1477), `src/links.rs` (486), `tests/unit/links.rs`, `scripts/fallback_test.py` | tcp, tls, ws, wss, quic, quicv2, kcp, plus raw carriers (`faketcp`, `l2tcp`, `icmp`, `spoof`) |
| Resume | `src/resume.rs` (997), `tests/unit/resume.rs` (508), `scripts/resume_test.py` | offset/cumulative-ACK per stream, SYNC on lane change, 16 MB window |
| Doctor | `src/doctor.rs` (903), `tests/unit/doctor.rs` | probes each carrier against the peer, ranks by measured speed, emits a config |
| Status / metrics | `src/status.rs` (1026), `tests/unit/status.rs` | RTT/jitter/loss/throughput per lane via local Unix socket |
| Limits / quota | `src/limits.rs` (647), `LIMITS.md` | back-pressure throttling, day/week/month quota persisted to JSON |
| DNS / routing | `src/dns.rs`, `src/route.rs`, `src/proxy.rs`, `CORE.md`, `DNS.md` | Rakhsh is also a SOCKS/HTTP proxy and router |
| Fleet / control | `src/fleet.rs`, `control/`, `CONTROL.md` | SSH-based fleet, Telegram bot, web panel |
| Crypto | `src/noise.rs`, `snow`, `ring`, `rustls` | Noise handshake; `encryption = "none"` mode exists (SECURITY.md) |
| Raw / L3 | `src/raw.rs`, `win_raw.rs`, `mac_raw.rs`, `l3.rs`, `tun-rs` | needs privileged networking |
| CI/CD | `.github/workflows/ci.yml`, `release.yml` | |

## 2. BAFT-TUNNEL architecture (current, verified from source)

Rust core, tokio, yamux over **one** authenticated stream per tunnel (`tcp-tls` TLS 1.3 mTLS, experimental `noise-tcp`), `baft` as separate Go engine process (ADR-0019), local agent over UDS with `SO_PEERCRED` (ADR-0023), protocol v2 with strict Option A (`PROTOCOL_V2_COMPATIBILITY_MATRIX.md`), authenticated Integrity probe (ADR-0028), Reverse Preview (ADR-0021/0037), single-session reconnect with 1 s to 60 s doubling backoff (`src/tunnel.rs` `INITIAL_BACKOFF`/`MAX_BACKOFF`), multi-tunnel per host (ADR-0036). Not present: second carrier per tunnel, stream resume, RTT/jitter/loss metrics, `doctor` command (preflight exists in the agent/setup path), quota/limits, DNS routing.

## 3. Capability matrix and decisions

Groups: **A** direct transfer, **B** adapt, **C** rewrite, **D** research, **E** incompatible with architecture, **F** unacceptable (security/legal). Because of section 0, nothing is group A.

| # | Capability | Rakhsh source | BAFT today | Gap | Decision | Prio |
|---|---|---|---|---|---|---|
| 1 | Multi-carrier failover/failback/backoff | `links.rs`, `carrier.rs`, LINKS.md | one link per tunnel; global reconnect backoff | no link list, no per-link penalty, no failback | **C** rewrite: pure link-policy module first (ADR-0039) | P0 |
| 2 | Stream resume | `resume.rs`, RESUME.md | none; session loss resets all streams | whole feature | **C** rewrite, needs **protocol v3** (ADR-0040); design now, build after multi-carrier | P0 |
| 3 | Transport health metrics | `status.rs`, `limits.rs` counters | session counters + integrity evidence | RTT/jitter/loss/state model | **C** rewrite: health model (ADR-0041) | P0 |
| 4 | Doctor / preflight | `doctor.rs` | setup preflight in agent | no `doctor` CLI, no machine-readable PASS/FAIL/WARN/UNKNOWN | **C** rewrite | P1 |
| 5 | Config validation | `config.rs`, `rakhsh check`, `tests/config_examples.rs` | `src/config.rs` validation | schema as single reference, unknown keys, deprecations | **C** | P1 |
| 6 | Control socket | CONTROL.md local Unix socket | UDS + `SO_PEERCRED` already | none material | **no change** (already stronger: peer-UID) | P1 |
| 7 | Quota / bandwidth limits | `limits.rs`, LIMITS.md | none | needs multi-tunnel accounting | **D** design only | P2 |
| 8 | DNS / split routing | `dns.rs`, `route.rs`, `proxy.rs` | none | turns BAFT into a general router | **E** out of scope without own ADR; optional module design only | P2 |
| 9 | Fleet (SSH + bot) | `fleet.rs`, `control/` | panel + agent per host | SSH exec model and Telegram bot | **E/F** SSH key custody and bot token model conflict with ADR-0013/0014 | n/a |
| 10 | faketcp / icmp / spoof / raw / L3 TUN | `raw.rs`, `l3.rs`, `tun-rs` | none | privileged | **F** requires `CAP_NET_ADMIN`/raw sockets (owner ban, ADR-0030) | n/a |
| 11 | `encryption = "none"` | SECURITY.md | none | | **F** plaintext path | n/a |
| 12 | QUIC / KCP / WS carriers | `carrier.rs` | none | ADR-0010 plans QUIC later | **D** future carriers plug into ADR-0039 interface, each needs its own ADR | later |
| 13 | ACME installer | `acme.rs` | ADR-0033/0034 already ship ACME | none | **no change** | n/a |
| 14 | Panel UI | `control/panel-ui` (React) | own panel (ADR-0032) | health/lane view | **B** only own work | after P0 |

## 4. Files eligible for transfer

None (section 0). Eligible for **idea** reuse only: the behaviours listed in section 3 as documented in `LINKS.md`, `RESUME.md`, `LIMITS.md`, `CONTROL.md`, `SECURITY.md`. Test *ideas* (kill lanes mid-transfer, hash compare, flapping link) may be reproduced as independently written tests.

## 5. Files transferred

None.

## 6. Code rewritten independently

Tracked in the PR list in section 10. Rule: author works from ADRs in this repo, not from Rakhsh source. The review for this analysis deliberately did **not** open the bodies of `carrier.rs`, `links.rs`, `resume.rs`, `doctor.rs`; only file layout, sizes, test names and user docs were read. This is a process control, not a legal guarantee.

## 7. Rejected

CAP_NET_ADMIN/raw/faketcp/icmp/spoof/L3 (owner ban), `encryption = "none"`, SSH fleet and Telegram control, general proxy/router mode, copying Rakhsh code, depending on `shabdiz` (unknown licence, separate repo), `rama` git-pinned dependency.

## 8. Licence compatibility records

| Module | Source licence | Dependencies | Transfer decision | Obligations if it were copied |
|---|---|---|---|---|
| Carrier manager (`links.rs`, `carrier.rs`) | AGPL-3.0-only (no per-file headers found) | `shabdiz` (unknown, not published in review), quinn, tokio_kcp, tokio-tungstenite | **Blocked** (code); idea reimplemented | Whole combined work under AGPL-3.0, source offer to network users, preserve notices |
| Resume engine (`resume.rs`) | AGPL-3.0-only | tokio, bytes, blake3 | **Blocked** (code); idea reimplemented, TCP-style offset/ACK is generic | same |
| Health monitor (`status.rs`) | AGPL-3.0-only | serde_json, sysinfo | **Blocked**; own design | same |
| Doctor (`doctor.rs`) | AGPL-3.0-only | carrier stack | **Blocked**; own design | same |
| Limits/quota (`limits.rs`) | AGPL-3.0-only | tokio | **Blocked**; design only | same |
| DNS/route/proxy | AGPL-3.0-only | rama (git rev) | **Rejected** (architecture) | n/a |
| Raw/L3/TUN | AGPL-3.0-only | tun-rs, libc | **Rejected** (security) | n/a |
| Fleet/control | AGPL-3.0-only | axum-class web stack | **Rejected** | n/a |

Copyright holder is `CluvexStudio` per `Cargo.toml`; no CLA or additional permission found. Dependency licences were not individually audited because no dependency is taken from Rakhsh. This is not legal advice; counsel review is recommended before any decision to copy code or relicense.

## 9. Security review (design level)

New attack surface from the P0 work, all user-space and inside the existing authenticated session: more outbound dials (bounded by backoff and a per-tunnel link cap), a second accept path for additional carriers (must reuse the same mTLS identity and authorization, ADR-0011), resume buffers (memory exhaustion: hard per-stream and per-tunnel caps, ADR-0040), replay/session confusion on resume (session id bound to the handshake transcript, strictly new protocol version, no silent downgrade). No new capabilities, no firewall/route/sysctl change, no plaintext path.

## 10. Integration results

| Item | Branch / PR | State |
|---|---|---|
| Analysis + ADR-0038..0041 | `claude/project-thread-o35er1` | this PR |
| Link policy module (P0), pure logic, not wired | PR #292 (`claude/project-thread-o35er1-link-policy`) | 16 unit tests, clippy clean, local |
| Health model (P0), pure logic, not wired | PR #293 (`claude/project-thread-o35er1-health`) | 13 unit tests, clippy clean, local |
| Resume (P0) | design only, ADR-0040 | no code |
| Doctor, config schema (P1), P2 quota/DNS | design only, ADR-0042 | no code; preflight already covers most checks |

## 11. Performance results

None yet. Link policy and health model are pure logic with no I/O; benchmarks will be reported on the PRs.

## 12. Remaining work

Resume implementation (protocol v3 and persistence decision), wiring link policy into `tunnel.rs` with a real second carrier (needs a second carrier implementation; today only one exists), LAB acceptance on shared hosts, panel lane view, P2 designs.

## Blockers needing the owner

B-1: answered 2026-11-17, see ADR-0043 (copy allowed only into an isolated AGPL unit; core stays proprietary; nothing copied today).
