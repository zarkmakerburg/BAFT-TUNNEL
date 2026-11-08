# Engine Integration Feasibility and Comparable LAB Plan (research, docs only)

- Status: research report v1.0, 2026-11-08. Design only. No engine added, no LAB job run (the shared LAB hosts are in use by other threads), no production server touched.
- Contract baseline: ADR-0018 (engine framework), ADR-0019 (BAFT as separate Go host), ADR-0020 (generic managed-runtime host, `src/runtime_host.rs`, `release/engine-manifest.json`), ADR-0021 (Reverse unavailable until LAB proof).
- Evidence labels: V/I/X/D/N/? as in `ENGINE_ECOSYSTEM_COMPARISON.md` section 0.

## 1. Question

Can BackPack run as an independent Engine through the existing generic Go Launcher (`runtime_host`) without changing the Rust core, the Capability Contract, ADR-0021 or the Direct engines? A standalone executable does not satisfy the contract automatically, so the contract is checked clause by clause.

## 2. Contract clauses versus BackPack

Reference build: BackPack `v1.9.0` (tag `dfd29d56546f`), checkout `9d58f84`, `go 1.26.9`, `CGO_ENABLED=0` static build (`Makefile:19`).

| Contract clause (source) | BackPack finding | Evidence | Result |
|---|---|---|---|
| Linux AMD64 | `make` builds `dist/backpack-linux-$ARCH` for several arches with `CGO_ENABLED=0 -trimpath`; pure-Go deps except optional eBPF loader (`cilium/ebpf`, used by the XDP spoof receiver only: `internal/utils/network/spoofxdp_linux.go`). | I (Makefile read, not built) | Compatible in principle |
| Ubuntu 22.04 / systemd 249 | Static Go binary, no libc dependency. Upstream unit uses only `Type=simple`, `Restart=always`, `RestartSec`, `LimitNOFILE` (`internal/manage/core/systemd.go:UnitFor`). All supported by systemd 249. Toolchain needs Go >= 1.26.9 at build time, which is not in Ubuntu 22.04 packages; build off-box in CI. | I | Compatible; build in CI |
| One process per instance | "One process per tunnel" is an explicit architectural rule (`docs/architecture.md`). Engine mode is `backpack -c <file>`; all management modes are separate flags. | V | Matches `ChildSpec{binary, run_args}` |
| Config generation | Single TOML file per tunnel; three engine tables (`[server]`/`[client]`, `[direct]`, `[l3]`) chosen by presence; `backpack check -c <file>` validates without starting (`internal/cli/cli.go:79,107`). | V | `validate_args = ["check","-c",file]`, `run_args = ["-c",file]`. Renderer can be a pure function with golden tests like `baft_engine` |
| Config path / state paths | `ConfigDir = "/etc/backpack"`, `/run/backpack` for the control socket (`enginectl.Dir`), metrics snapshot `<dir>/<name>.metrics.json`. Constants, no env override found; `-c` can point elsewhere but the socket directory stays fixed. | I | Adapter must provision `/run/backpack` (0700 root) or the engine must be patched; two engines of different names do not collide |
| Environment | Reads `NO_COLOR` only on menu paths (`main.go:62`). Host uses `env_clear()` and null stdin (`runtime_host.rs:194-200`). | I | Compatible |
| Runtime process model | Single process, goroutines, SIGINT/SIGTERM handled with context cancel then wait (`main.go:127-150`). No daemonize, no sd_notify in tunnel units (`Type=simple`). | V | Matches host: SIGTERM then SIGKILL after `grace` |
| Graceful shutdown | `cancel(); <-done` in `runEngine`; bounded by generation teardown. Time to drain is not documented. | I | `grace` must be measured in LAB |
| Readiness contract | Host contract is `tcp_listen` on a local address (`ReadinessContract::TcpListen`). Server role listens on the tunnel bind port and forwarded ports, so TCP accept works. **Client (dialing) role has no local listener in reverse mode**, so `tcp_listen` cannot be satisfied. | V (host) / I (BackPack) | **Gap 1** |
| Health | Engine answers `Status` over a unix socket (0600 in a 0700 dir): role, transport, `connected`, traffic, generation (`cmd/enginesocket.go`), plus a metrics JSON refreshed every 30 s. Control channel state is `connected`, which is the signal ADR-0020 says TCP accept does not give. | I | Needs a new readiness kind (see 3.1) |
| Restart / recovery | Engine recovers in-process (generation lifecycle, ADR 0001/0005). Host restart budget and unit `Restart` still apply. Process restart loses in-flight flows (as with BAFT in ADR-0019). | V | Compatible |
| Multi-instance | Each tunnel = its own process, named config, named socket and metrics file, own ports (`app.ConfigPath(name)`). | I | Compatible with port allocation done by the Rust renderer |
| Peer identity (ADR-0018 clause 1, fail-closed, node-ID mapping, CRL) | **Shared token only.** Constant-time compare (`announce.go:102`), Noise NNpsk0 for `stealth` is keyed on the same token. No `ClientAuth`/mTLS anywhere in the tree (searched), no per-peer identity, no revocation list. | V (code searched) | **Gap 2: cannot satisfy ADR-0018 clause 1 as written** |
| Security boundaries | Upstream unit has no `User=`, no hardening; engine runs as root and uses raw-socket features (spoof, XDP, l3 TUN) if configured. Binary also contains menu, web panel, Telegram bot, monitor, SSH fleet, self-update and geo lookups, none of which the engine path needs. Outbound-call sites (`api.github.com`, geo) are in `internal/manage`, `internal/node`, `internal/geo`, not in `cmd/` engine path (grep). | I | Gap 3: run with hardening and egress limited to the peer, prove by LAB network-deny test |
| License | AGPL-3.0 plus s.7 additional terms in `NOTICE` (attribution line + trademark). Separate process, no linking; same model as ADR-0019. See `ENGINE_LICENSE_AND_DEPENDENCY_MATRIX.md`. | V (files read) | Acceptable only with the obligations listed there accepted |
| Source and attribution | Must keep `Based on BackPack by Amin Mohammadi (AminMGMT) https://github.com/AminMGMT/BackPack` visible in the distribution's NOTICE and README when conveying a modified version or running one for others over a network; trademark "BackPack" not licensed. NOTICE itself says it also carries upstream AGPL/GPL third-party attribution for the data plane. | V | Obligation, no technical blocker |
| Reproducible build | Makefile has a `repro` check (two builds with `-trimpath`, CGO off) (`Makefile:83-90`). Not run. Module list is large (quic-go, kcp-go, wireguard, utls, ebpf, gopsutil). | I | Feasible; verify with two independent builds and equal SHA-256 before pinning |

### 2.1 Verdict

BackPack is the strongest independent-Go-Engine candidate by features (12 reverse transports, fallback chain, mux, UDP, process-per-tunnel, tests), but it does **not** satisfy the Runtime Host contract today. Three gaps must be closed first, in this order:

1. **Readiness for a dialing role.** (Gap 1)
2. **Identity model.** (Gap 2)
3. **Hardening and egress proof.** (Gap 3)

None of the gaps needs a change to the Rust core's engines, and none touches ADR-0021.

## 3. Gap closure proposals (design only)

### 3.1 Gap 1: readiness

Extend `ReadinessContract` (ADR-0020 manifest schema, `deny_unknown_fields`) with a second kind, for example `unix_status`: the adapter names a socket path and the host connects, sends the engine's `status` op, and reports `Ready` only when the JSON answers `connected: true` (dialer) or listener bound (server). Host stays generic: the probe is "connect to this path, expect this JSON field", not BackPack knowledge. Requires a manifest `schema_version` bump and tests, so it is a small separate PR. Until then, BackPack can only be hosted in a role that has a local listener (server side, or a Direct engine ingress).

### 3.2 Gap 2: identity

Options:

| Option | What it means | Verdict |
|---|---|---|
| A. New capability class | Add a capability flag to the contract, e.g. `peer_identity = shared_secret`, `revocation = rotate_only`, `authorization_enforcer = engine`. Wizard and preflight report a **WARNING** ("authorization is a shared secret, not per-node; revocation needs rotation and restart"). | **Recommended**, honest and cheap |
| B. Derive per-tunnel secrets | The Rust core derives each tunnel's token from node key material (HKDF with tunnel id and both node IDs) and renders it into a 0600 file; compromise of one tunnel's token does not expose others. Still not per-peer revocation. | Recommended with A |
| C. Carrier-only sidecar | Use BackPack only as a byte carrier under the Rust core's own authenticated session (ADR-0019 option A2). Strongest security, but BackPack's pairing/pool/control model owns the streams, so it is not a clean carrier. | Reject for BackPack; fits better for a WebSocket/QUIC carrier built on a library |
| D. Wait for upstream mTLS | Not planned in the tree. | Do not rely on |

This needs its own ADR (proposed number ADR-0022) before any BackPack manifest entry is created, because it narrows ADR-0018 clause 1 for a second engine. ADR-0019 already did so for BAFT on the grounds that BAFT enforces mTLS; BackPack cannot make the same claim.

### 3.3 Gap 3: hardening and egress

Adapter-provided systemd drop-ins (all available in 249): `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ReadWritePaths=` limited to engine state and `/run/backpack`, `PrivateTmp=yes`, `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX` (blocks AF_PACKET/raw spoof carriers), `CapabilityBoundingSet=` empty unless a port below 1024 needs `CAP_NET_BIND_SERVICE`, `User=` an engine user. Which of these BackPack tolerates (notably `/run/backpack` creation and 0600 key reads as non-root) is **unknown (?)** and must be tested in LAB. Egress proof: run with only the peer address allowed and confirm no other connection attempts (`ss`/nftables counters) during start, idle and reload.

## 4. Other candidates as engines (short)

| Candidate | Engine fit | Blocking gap | Note |
|---|---|---|---|
| chisel (MIT) | Good: one process, `chisel server --reverse` / `chisel client`, SSH host-key fingerprint pin, per-user regex permissions, WebSocket transport (useful camouflage), TCP+UDP. | Readiness for client (no listener); user:password auth is a secret, not node identity. Fingerprint pin can be derived from a Rust-issued key. | Best licence (MIT). Candidate for WebSocket-cover Reverse engine. |
| frp (Apache-2.0) | Mature, many protocols, dashboard. | Large attack surface (HTTP vhosts, plugins, dashboard) vs our one-port wizard; token auth md5-based. Optional mTLS exists. | Reference and possible later engine; heavy. |
| rathole (Apache-2.0) | Small, Noise/TLS/WebSocket. | Rust: could be built as a **crate or fork** rather than a process; token auth per service; no mux. | Use as design reference for the native Rust Reverse. |
| spoof-tunnel, Pahlavi, scripts | Not engines. | See comparison doc. | Excluded. |

### 4.1 C-BAFT (`ctunnel`)

Checked against the same clauses (source in `ENGINE_ECOSYSTEM_COMPARISON.md` 2.1):

| Clause | Finding | Result |
|---|---|---|
| One process per instance | `ctunnel-engine@<name>.service`, `-config <env file>` | Matches for userspace transports only |
| Config | Env file per tunnel; created from a `ctunnel://` link | Renderer feasible |
| Host effects | Kernel L3/L2 methods run `ctunnel-kernel up` (ip/iptables/sysctl) before the engine | **Conflicts with "no hidden host effects"**; would need a host-change preview like the wizard preview (PR #75) |
| Readiness | No listener contract for userspace UDP; no status socket seen | Needs the `unix_status` kind or a wrapper |
| Identity | Shared secret, HMAC-derived AES-GCM subkeys, no revocation | Same class as BackPack (ADR-0022) |
| Licence | AGPL-3.0, CIAUB copyright, BackPack-ported files | Separate process only; owner acknowledgement D5 |
| Build | Go 1.22, no module dependencies | Easiest build and audit of the Go candidates |

Verdict: not a first-wave engine. Reuse as ideas: measured Connection Test (real protocol probes with measured MTU), `rudp-fec`, setup-link provisioning.

## 5. Do not assume list (explicit checks before any manifest entry)

- A running process is not a healthy tunnel (ADR-0020 clause 5). For BackPack, `connected` from the status op, plus a Rust-side end-to-end byte probe, are both required before `healthy`.
- Binary SHA-256 pinned in `release/engine-manifest.json` must come from our own reproducible build of a tagged source commit, never from an upstream release asset or `install.sh`.
- No upstream `install.sh`, panel, monitor, Telegram or node/SSH fleet code runs on a BAFT-TUNNEL host.
- Wizard exposes an engine only when its adapter, manifest entry, readiness contract and LAB evidence exist (mission rule; ADR-0018 clause 6).

## 6. Comparable LAB plan (design only; not executed)

Principle: one test matrix, identical for every candidate, on the same two hosts, with the candidate under test as the only variable. This is a measurement plan, not a ranking; no result is claimed.

### 6.1 Environment record (captured once per run into a JSON evidence file)

Both hosts' kernel (`uname -r`), distro, systemd version, CPU model and count, RAM, NIC and MTU, public IPs (redacted to labels), clock sync state, `sysctl` for `net.core.rmem_max`, `net.ipv4.tcp_congestion_control`. Candidate: name, upstream tag, source commit SHA-40, **our** binary SHA-256, build command, Go/Rust toolchain versions, rendered config SHA-256 (secrets excluded), unit file SHA-256. Load generator versions (`iperf3 --version`), kernel qdisc state. Secrets never logged.

### 6.2 Isolation

Dedicated LAB instances only; each run in its own systemd transient unit and its own network namespace or port range; a cleanup phase asserts no leftover units, listeners, nftables rules or `tc` qdiscs; `cleanup_stale` semantics as in existing LAB jobs. No upstream installer is executed; binaries come from our CI build.

### 6.3 Matrix

Each case is run for each candidate and each applicable transport, direction (Direct, and Reverse only once the candidate supports it and ADR-0021 permits testing it), three repetitions, median and range reported.

| Case | Method | Measured |
|---|---|---|
| Clean route baseline | `iperf3 -c` and `ping -c 100` host to host without tunnel | Reference ceiling and RTT |
| Latency and jitter | `tc netem delay 50ms 10ms distribution normal` on the path; ping and TCP request-response probe through the tunnel | Added RTT p50/p95/p99 and jitter versus baseline |
| Packet loss | `tc netem loss 1%` and `5%`; 60 s each | Goodput, retransmit counts, stalls > 2 s |
| TCP throughput | `iperf3 -P 1,4,16 -t 60` through the tunnel | Mbit/s, host CPU for both processes |
| UDP throughput | `iperf3 -u -b {10,100,500}M -t 60`; record loss, jitter, reorder | Delivered rate, loss, only for UDP-capable candidates; others UNKNOWN/N |
| Many concurrent connections | 100, 1000, 5000 parallel short TCP flows with a fixed payload | Success ratio, p99 connect time, fd and memory growth |
| Reconnect after interruption | (a) kill candidate process on one side; (b) nftables drop (no RST) for 30 s; (c) RST/FIN cut; (d) peer restart | Time to detect, time to first byte after restore, whether manual action needed |
| CPU and RAM | `pidstat -u -r -h 1` during every case; idle 10 min | CPU-seconds per GiB moved, RSS peak, RSS after idle |
| Byte integrity | 64 KiB, 2 MiB and 1 GiB random payloads, SHA-256 both ends, both directions, plus concurrent mixed sizes | 0 mismatches required; any mismatch = FAIL for that candidate |
| Multi-hour stability | 6 h and then 24 h soak at 10 % of measured ceiling with periodic integrity probes and a 1-minute netem disturbance each hour | Disconnect count, leaked fds/RSS slope, recovery times |
| Security negatives | Wrong identity or secret, replayed handshake, wrong pin; occupied port; revoked identity (where supported) | Refused with no data; otherwise recorded as capability gap |
| Egress audit | Only peer allowed outbound; log denied attempts | No unexpected connections |

### 6.4 Reporting rules

- Each cell is PASS, WARNING, BLOCKED or UNKNOWN with the evidence file name; no number is shown without its environment record.
- Differences within run-to-run variance are reported as "not distinguishable".
- Candidates are never ranked across different transports or security models: a shared-secret token engine and a mutual-TLS engine are different classes (section 3.2) and are reported side by side, not as a leaderboard.
- LAB scheduling: the shared LAB hosts are in use. This plan is only to be executed by the thread that owns LAB access, one run at a time, after the P-priorities in `ENGINE_IMPLEMENTATION_PROPOSAL.md`.
