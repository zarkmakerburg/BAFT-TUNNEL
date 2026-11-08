# Engine Ecosystem Comparison (research, docs only)

- Status: research report v1.0, 2026-11-08. No code integrated, nothing installed, no server touched, no LAB job run.
- Tracking: HQ Issue #2. Related: ADR-0018, ADR-0019, ADR-0020, ADR-0021.
- Siblings: `REVERSE_ARCHITECTURE_COMPARISON.md`, `ENGINE_INTEGRATION_FEASIBILITY.md`, `ENGINE_LICENSE_AND_DEPENDENCY_MATRIX.md`, `ENGINE_IMPLEMENTATION_PROPOSAL.md`.

## 0. Method and evidence labels

Every upstream repository was cloned read-only (`git clone --depth 50`) into an isolated scratch directory. Only source, manifests and docs were read. No upstream installer, `install.sh`, `curl | bash` script or binary was executed, and no upstream test suite was run. "Tested" below therefore means "test files that exercise the path exist in the repo", not "we ran them".

| Label | Meaning |
|---|---|
| **V** | Implemented. Source path read, and test files covering it exist upstream (not run by us). |
| **I** | Implemented. Source path read, no covering test located. |
| **X** | Upstream itself labels it alpha, beta or experimental. |
| **D** | Documented only. README or docs claim it, no code path found. |
| **N** | Absent or explicitly unsupported (searched). |
| **?** | Not determined in this pass. |

README marketing text is never used as evidence on its own. Where a claim is only from a README it is labelled D.

## 1. Repository inventory (verified)

| Repository | Language / kind | Pinned HEAD (default branch) | Latest release tag | Last commit | License | Verdict |
|---|---|---|---|---|---|---|
| AminMGMT/BackPack | Go 925 files, 481 `_test.go` | `9d58f8449ac0` (VERSION file `v1.9.0`) | `v1.9.0` -> `dfd29d56546f` | 2026-10-19 | AGPL-3.0 + s.7 terms (NOTICE) | Real engine, active |
| rathole-org/rathole | Rust 20 files, ~3.1k LOC | `a292f7ed5402` | `v0.5.0` -> `ebb764ae53d7` | 2026-08-23 | Apache-2.0 | Real engine, slow cadence |
| fatedier/frp | Go 383 files | `d42812ae0eb6` | `v0.72.0` -> `bafec21abce5` | 2026-11-05 | Apache-2.0 | Real engine, very active |
| jpillora/chisel | Go 85 files | `5ee6c74d60d9` | `v1.13.0` (= HEAD) | 2026-10-12 | MIT | Real engine, active |
| ParsaKSH/spoof-tunnel | Go 27 + Rust 14 files, eBPF | `ba29f0c8e22d` | `v3.1.0-beta.0` | 2026-05-22 | Apache-2.0 | Real, beta, high risk |
| ParsaKSH/TAQ-BOSTAN | bash installers + 1 Python monitor | `264cec99b23a` | `v2.1.0` | 2025-12-07 | Apache-2.0 | Scripts only, no engine |
| Schmi7zz/Phormal | one 6,459-line bash script | `65090c2e820d` | none | 2026-07-22 | GPL-3.0 | Orchestrator of other engines |
| Zehnovik/Pahlavi-tunnel | 422-line Python + bash | `00191506e7c1` | none | 2026-02-27 | **no license file** | Unauthenticated toy, do not use |
| Musixal/rathole-tunnel | bash menu + committed binaries | `ed49cdd88945` | none | 2025-06-15 | GPL-3.0 | Installer for rathole, binaries unverifiable |
| M-R-R-X/ctunnel | - | - | - | - | - | **Not verified, excluded** |

M-R-R-X/ctunnel: `git clone https://github.com/M-R-R-X/ctunnel` failed with an authentication prompt (the GitHub response for a missing or private repository). A web search for the exact name returned only `alienrobotarmy/ctunnel`, an unrelated 2009-era C tool by a different author. Per the mission rule it is excluded from every table. If the owner has the exact URL, re-run this check.

## 2. What each project actually is

**BackPack (Go).** One binary, three engines chosen by config table: reverse (`[server]`/`[client]`), direct (`[direct]`), layer-3 (`[l3]`) (`docs/architecture.md`, `cmd/cmd.go runEngine`). One process per tunnel, one systemd unit per tunnel (`internal/manage/core/systemd.go:UnitFor`, `Type=simple`, `Restart=always`, `LimitNOFILE=1048576`). Reverse has 12 transports (`tcp, tcpmux, stealth, ws, wss, wsmux, wssmux, kcp, quic, udp, xdi, pck`), a signal-byte control channel, a pre-opened pool of data connections, and a "generation" lifecycle that survives client re-dials (ADR 0001, 0005 in `docs/adr/`). Management (menu, web panel, Telegram, monitor, SSH fleet) is bundled in the same binary and is not needed for an engine.

**rathole (Rust).** Small (3.1k LOC) NAT-traversal reverse proxy. Server listens; client dials; one control channel per service; one fresh TCP data connection per user connection (no multiplexing). Transports: tcp, tls (native-tls or rustls, feature flags), noise (`snowstorm`, default `Noise_NK_25519_ChaChaPoly_BLAKE2s`), websocket. UDP forwarding. Hot reload. Explicitly out of scope: HTTP routing, logging, visitor-side setups (`docs/out-of-scope.md`).

**frp (Go).** Mature platform. `frps` listens, `frpc` dials out and registers proxies. Control connection + work-connection pool, optional yamux multiplexing (default on), protocols tcp/kcp/quic/websocket/wss, TLS on by default for `frpc`, token or OIDC auth, dashboard, Prometheus, plugins, health checks, load-balancing groups, P2P (xtcp), alpha VirtualNet (TUN).

**chisel (Go).** HTTP/WebSocket transport carrying an SSH session (`crypto/ssh`). Client dials the server, sends a `config` request listing remotes; reverse remotes (`R:`) make the server listen and send traffic back over SSH channels. TCP and UDP remotes, SOCKS5 incl. UDP, client auto-reconnect with backoff, application-level ping with timeout (fix tracked in `TASKS.md` item 1), host-key fingerprint pinning, per-user regex permissions.

**spoof-tunnel (Go+Rust).** From v2 a "UDP pipe" whose packets carry forged IPv4/IPv6 source addresses over raw sockets (ICMP, ICMPv6, UDP, TCP-SYN), optional XDP receiver. Upstream README (v3+ section): FEC, ChaCha20, SOCKS and the ACK layer "have been removed from the core"; the recommended pattern is WireGuard over the pipe. So the core pipe provides no payload encryption of its own (D from README; no crypto package found in `internal/` or `rust/`).

**TAQ-BOSTAN.** Three bash installers: Hysteria2 (pinned `app/v2.6.1` binary downloaded from apernet releases, optional salamander obfs, iptables mark-based monitor), SIT-tunnel private IPv6 via netplan, WireGuard private IPv6. Nothing to embed; ideas only.

**Phormal.** A bash orchestrator that installs and wires six "products": Bridge, Relay (gost/hysteria downloads), Reverse (= rathole, per `phormal.sh:44,512-560`), GRE, Echo (ICMP), Raw. Its valuable part is the Path Test: from one host it SSHes to the peer, runs real bidirectional traffic per product, prints PASS/FAIL with a confidence rating and picks a "BEST CHOICE" by `confidence*100 + static product rank` (`layer_autotest_verdict`, `layer_autotest_product_rank`: Relay 92, Bridge 88, GRE 80, Reverse 75, Echo 70, Raw 65). The ranking is a hard-coded preference, not a measurement.

**Pahlavi-tunnel.** Python reverse TCP bridge. `Pahlavi.py` has no authentication, no encryption and no TLS: any host that connects to the bridge port is treated as the EU peer and receives user traffic; any host that connects to the sync port can make the IR side open arbitrary listening ports (`ir_mode.sync_listener` -> `open_port`). Source has no license. Not a candidate for anything beyond the UX idea of "auto-sync of ports".

**rathole-tunnel.** Menu installer for upstream rathole. Downloads the latest upstream release by scraping the GitHub API, or falls back to committed zips `core/rathole*.zip` whose provenance is not recorded ("modified" builds from 2024-07). Nothing to take.

## 3. Capability matrix

Columns: BackPack (BP), rathole (RH), frp, chisel (CH), spoof-tunnel (SP). Script-only projects (TAQ-BOSTAN, Phormal, Pahlavi, rathole-tunnel) are in section 4. BAFT-TUNNEL's own engines are in the last column for reference (from `docs/ADR-0018`, `ADR-0021`, `src/tunnel.rs`).

| Capability | BP | RH | frp | CH | SP | BAFT-TUNNEL today |
|---|---|---|---|---|---|---|
| TCP | V | V | V | V | I (carries TCP only via WireGuard on top) | V (tcp-tls, noise-tcp) |
| TLS 1.3 | I (reverse wss uses Go server defaults; 1.3 floor enforced only in l3 QUIC, `internal/tunnel/l3/carrier_quic.go:149,559`) | I (native-tls/rustls, floor depends on build) | I (Go defaults, `frpc` TLS default on: `pkg/config/v1/client.go:185`) | I (optional https/wss in front) | N | V (mutual TLS) |
| Noise | V (`stealth`, NNpsk0 keyed on the shared token, `internal/server/transport/tcp.go:265`; `flynn/noise`) | V (`Noise_NK` default, `src/transport/noise.rs`; integration test config `tests/for_tcp/noise_transport.toml`) | N | N | N | V (noise-tcp) |
| WebSocket | I (`ws.go`, `wsmux.go`) | I (`transport/websocket.rs`, test configs) | V (`protocol = websocket`) | V (only transport) | N | planned (ADR-0018 #4) |
| WSS | I (`wss`, `wssmux`) | I (`websocket-native-tls` feature) | V | V (behind TLS terminator or `--tls-*`) | N | planned |
| QUIC | I (`quic.go`, quic-go) | N | I (`quicBindPort`, quic-go) | N | N | planned |
| KCP / FEC | I (`kcp.go`; FEC via `kcp_datashards` `config/config.go:80`) | N | I (`kcpBindPort`) FEC not located | N | N (removed from core per upstream) | N |
| UDP forwarding | I (`udp` transport, `docs/forwarded-udp.md`) | V (`StartForwardUdp`, `tests/for_udp/*`) | V (udp, sudp proxies) | V (`/udp` remotes, `udpsocks`; tests `tunnel_out_ssh_udp_test.go`) | V (its whole purpose) | N |
| Multiplexing | V (`tcpmux`, `wsmux`, `wssmux` via smux) | N (one TCP per user flow) | V (yamux, default on: `pkg/config/v1/server.go:185`) | V (SSH channels over one connection) | I (own master session, StreamID, v1 layer removed in v2+) | V (yamux) |
| Reverse (service side dials out) | V (primary mode) | V (primary mode) | V (primary mode) | V (`--reverse`, `R:` remotes) | I (symmetric spoof, not NAT reverse) | **N until LAB gate** (ADR-0021; core code from PR #76 exists but is not advertised) |
| Direct (ingress side dials) | V (`[direct]`, `internal/tunnel/direct`) | N | N (xtcp P2P only, I) | V (non-R remotes) | I | V |
| Layer 3 | I (`[l3]`, TUN, Noise NNpsk0, 2048 replay window) | N | X (VirtualNet ALPHA, `README.md:1373`) | N | I (via WireGuard on top) | N |
| Failover / multi-server | I (`fallback_transports`, `config/fallback.go`; manage fleet) | N | I (load-balance groups, health checks) | N | N | N |
| Transport fallback | I (same list on both ends, dwell-based meeting, `docs/transport-fallback.md`) | N | N | N | I (asymmetric per-direction transport) | planned (ADR-0018 clause 2) |
| IPv6 | I (`Listen on IPv6 as well`, l3 carries v6) | I (`prefer_ipv6` client option, `client.rs:258`) | I | I | V (ICMPv6, IPv6 headers) | ? |
| Recovery / reconnect | V (generation lifecycle, `lifecycle_test.go`, `restart_test.go`) | V (backoff, `run_control_chan_backoff`, retry tests via integration) | V (loginLoop + FastBackoff 1s..20s, runID replace, `service_lifecycle_test.go`) | V (backoff + keepalive timeout, `client_retry_test.go`) | ? | V (backoff 1s..60s, CRL reload) |
| Peer authentication | shared token, constant-time compare (`announce.go:102`); no per-peer identity, no mTLS (no `ClientAuth` in tree) | per-service shared token, challenge `sha256(token‖nonce)` (`server.rs:276-330`); compare is plain `!=` on digests | shared token (`md5(token‖timestamp)`, const-time compare, `pkg/auth/token.go`) or OIDC; optional mTLS via `trustedCaFile` (`common.go:100`) | SSH host-key fingerprint (client verifies server); users file with per-user regex permissions (server verifies client) | pre-shared base64 key, no encryption in v2+ core | per-node identity: mutual TLS or Noise static key + CRL |

## 4. Script-only projects

| Project | Capability | Evidence |
|---|---|---|
| TAQ-BOSTAN | Hysteria2 (QUIC) + salamander obfs, port mapping, SIT and WireGuard private IPv6 over netplan | Scripts only (`hysteria.sh`, `sit.sh`, `wireguard.sh`); some client configs generated with `insecure: true` (`hysteria.sh:576`) |
| Phormal | Per-product path test, best-product recommendation, port-hopping Relay, guard keepalive units | Bash only; downloads binaries by URL (`phormal.sh:294-302`), no pinned hash seen in grep |
| Pahlavi | Multi-slot configs, auto port sync, health check, BBR tuning | Python/bash; **no auth, no encryption** |
| rathole-tunnel | Menu around rathole | Bash; committed binaries |

## 5. Product ideas worth adapting

| Idea | Source | Value | Risk | Verdict |
|---|---|---|---|---|
| Route diagnostics with real traffic per transport before choosing | Phormal Path Test, BackPack `internal/manage/conntest*.go` | High: lets the wizard say PASS/WARNING/BLOCKED/UNKNOWN from measurements | Needs SSH or agent-to-agent probing; must be non-destructive | **Adopt as design**, build on existing preflight and agent API |
| Transport fallback list, same list on both ends, dwell-based rendezvous | BackPack `docs/transport-fallback.md` | High for ADR-0018 clause 2 | Two ends must be configured together | Adopt the idea, implement natively in Rust supervisor |
| Control channel + on-demand data stream pool, nonce-bound data channels | rathole, frp | Direct input to ADR-0021 | rathole's data-channel hello carries the session digest in clear on plain TCP (bearer) | Adopt structure, bind data streams to the existing authenticated session instead |
| Service mapping (name -> bind address, per-service token) | rathole, frp | Medium; maps to "one port field" wizard | Per-service tokens conflict with per-node identity | Adopt service names, keep node-identity auth |
| Runtime port allowlist and bandwidth limits | frp (`allowPorts`, limits) | Medium | - | Later |
| Session/run ID that makes a reconnect replace the stale control | frp `ctlsByRunID`, rathole "drop previous control channel" | High: removes the stale-session reconnect bug class | - | **Adopt in ADR-0021 implementation** |
| Application-level ping with timeout | chisel (`TASKS.md` item 1), frp heartbeat timeout | High: dead-path detection | - | Verify our yamux keepalive covers it |
| Host-key fingerprint pin + per-user permission regex | chisel | Mirrors our CA-pin/ACL model | - | Already covered by ADR-0011/0012 |
| Process-per-tunnel, status via local 0600 unix socket, metrics snapshot file | BackPack `internal/enginectl`, `docs/architecture.md` | High for the Runtime Host contract (readiness beyond TCP accept) | - | Adopt as contract extension (see Feasibility doc) |
| Multi-tunnel list with sync of exposed ports | Pahlavi | Idea only; its implementation is unsafe | Unauthenticated | Do not copy; the idea is the "port list is desired state" model we already have |
| IPv6 private overlay via WireGuard or SIT | TAQ-BOSTAN | Low for now; touches netplan/system state | Host network changes | Later, behind ADR |
| Spoofed-source carriers | spoof-tunnel | See risk | See below | **Do not integrate** |

### Spoof Tunnel: feasibility and operational risk only

- Requires both hosts to be able to emit forged source addresses (raw sockets, `CAP_NET_RAW`, optional XDP/eBPF). Many providers drop spoofed egress (BCP38), so it fails silently on exactly the hosts where it is needed or gets the VPS account terminated.
- Core pipe has no encryption or authentication of its own in v2+ (upstream's own text); all security would be delegated to a second layer (WireGuard), which ADR-0018 notes is blocked in the target environment.
- It forges addresses belonging to third parties, which is abusive to the owners of those addresses and legally risky for the operator. It collides with BAFT-TUNNEL's "honest states, no hidden host effects" principles.
- Beta (`v3.1.0-beta.0`), Rust FFI + eBPF raises build and Ubuntu 22.04 kernel dependencies.

Conclusion: not an engine for BAFT-TUNNEL. Its `spoof-tester` idea (checking whether a path allows forged source addresses) belongs in a diagnostics tool at most, and only with explicit operator consent.

## 6. Recommendations (answers to mission section 9)

| Question | Recommendation | Basis |
|---|---|---|
| Best Reverse architecture reference | **rathole** for the control-channel + on-demand data-channel lifecycle (smallest and clearest); **frp** for session identity (runID), reconnect replace semantics and mux choice. See `REVERSE_ARCHITECTURE_COMPARISON.md`. | Code trace |
| Best candidate for an independent Go Engine | **BackPack**, but only after three gates: identity model (shared token is weaker than ADR-0018 requires), readiness contract extension (dialing role has no local listener), AGPL obligations accepted. Fallback candidate with the cleanest contract fit: **chisel** (MIT, SSH host-key pin, user permissions) as a Direct/Reverse engine behind WebSocket. | `ENGINE_INTEGRATION_FEASIBILITY.md` |
| Best native Rust Engine reference | **rathole** (Apache-2.0, Rust, Noise + TLS + WebSocket, 3k LOC, readable) as design reference. Code reuse is allowed by license but we should re-implement against our authenticated session. | Source |
| Best transport and fallback ideas | BackPack's fallback chain + dwell rendezvous; frp's QUIC/KCP/WSS selection by config; chisel's HTTP/WebSocket cover. | Source |
| Best operational management ideas | Phormal Path Test + BackPack health check and process-per-tunnel/unix-socket status; frp Prometheus and dashboard as later ideas. | Source |
| Engines that should not be integrated | spoof-tunnel (risk), Pahlavi-tunnel (no auth, no license), rathole-tunnel (unverifiable binaries), TAQ-BOSTAN and Phormal scripts as production infrastructure (host-mutating installers; ideas only). | Source |
| License and distribution risks | BackPack AGPL-3.0 plus NOTICE s.7 attribution and trademark; Phormal and rathole-tunnel GPL-3.0; Pahlavi unlicensed. See `ENGINE_LICENSE_AND_DEPENDENCY_MATRIX.md`. | License files |

Owner's closing view (rathole as the Reverse base, BackPack as the leading Go candidate, Phormal for route testing, frp as operational reference) is supported by the evidence with two adjustments: rathole is a **design reference** for our own Rust Reverse, not an engine to embed; and BackPack's candidacy is conditional, not yet feasible under today's contract.

## 7. Limits of this report

- Nothing was executed, so no performance statement is made. The only benchmark found (rathole `docs/benchmark.md`, 2021-12-28, commit `1180c7e`, localhost, frp 0.38.0) is five years old, single-host, and is not used as evidence.
- Test-file presence is not test success. CI status of upstream heads was not checked (no API access to those repositories from this session).
- Some "I" cells (QUIC, KCP, WSS in BackPack; IPv6 everywhere) were confirmed from config and transport files but not traced end to end.
