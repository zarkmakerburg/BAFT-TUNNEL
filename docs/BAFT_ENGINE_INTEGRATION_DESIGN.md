# BAFT Main Engine Integration: Investigation and Design (v1.0)

- Status: Proposed design, docs only. No runtime change, no engine removed, nothing merged beyond this document.
- Date: 2026-11-08
- Roadmap item: P2-002 (BAFT main engine), engine #3 in the ADR-0018 order.
- Related: ADR-0003 (Rust core), ADR-0010, ADR-0011, ADR-0013, ADR-0015, ADR-0017, ADR-0018; proposed ADR-0019 (`docs/ADR-0019-baft-engine-host.md`).
- Tracking: Issue #2 (HQ Control Board).
- Persian summary: section 1 ends with a Persian paragraph.

## 0. Evidence labels

| Label | Meaning |
|---|---|
| **[SRC]** | Verified by reading source at the SHA shown. |
| **[BUILD]** | Verified by running the command in this investigation (local, not CI). |
| **[CI]** | Verified by a CI run recorded in the repo docs (not re-run here). |
| **[LAB]** | Verified on the real two-host LAB (run recorded in repo docs). |
| **[INF]** | Inferred or proposed. Not verified. |
| **[NV]** | Not verified. |
| **[BLK]** | Blocked. |

Nothing in this document claims that a BAFT engine inside BAFT-TUNNEL works. It does not exist yet.

## 1. Executive summary

Recommendation: integrate BAFT as a **separately packaged Go "engine host" process that Rust configures, starts, supervises and probes, while the data plane stays entirely inside Go** (Method A, variant "managed process, no payload IPC"). Rust keeps the core, the identity/PKI, the config of record and the lifecycle. Alternative if the owner prefers Rust to see every stream: carrier-only sidecar (variant A2), which discards most of BAFT's value.

Why (evidence in sections 4 to 9):

1. BAFT is not a stream engine. It owns its own multiplexing (Session and Flow, `internal/session`), flow control, replay and recovery, route table and listeners. The current Rust trait `StreamEngine` (`src/engine.rs:29`) only secures one socket and leaves Yamux, handshake and authorization to `src/tunnel.rs`. BAFT cannot be expressed as a `StreamEngine` without throwing away what makes it BAFT. **[SRC]**
2. BAFT's runtime has **no external control API**. `management.unix_socket` is required by config validation but is only used to derive a telemetry spool filename; nothing serves it (`internal/config/config.go:246`, `internal/node/node.go:606`). Its only external surfaces are the config file, SIGHUP (revocation and config reload, `cmd/baft/main.go:174-187`) and a loopback Prometheus metrics endpoint. **[SRC]** A "Rust calls Go over IPC per stream" design would need new Go code first.
3. Keeping payload in one process avoids an extra copy and an extra failure domain on every byte.
4. Licensing: BAFT is **AGPL-3.0**; BAFT-TUNNEL is `LicenseRef-Proprietary` and its `deny.toml` allows only permissive licences. A process boundary with a source offer is the lowest-risk shape (section 15). **[SRC]**

Honest costs: authorization for this engine is enforced by BAFT's own fail-closed code from a Rust-generated config, not per stream by Rust (this narrows the ADR-0018 clause "Rust core is the only authorizer", see section 11). BAFT process-restart resume is not implemented upstream, so a Go crash resets active flows. The tcp-tls and noise-tcp engines stay untouched.

First executable commit (section 19): a pure-Rust config renderer plus a golden test and a CI step that runs the pinned `baft config validate` on its output. It starts nothing.

خلاصه فارسی: BAFT به‌صورت یک پردازش Go جدا (engine host) بسته‌بندی می‌شود که هسته Rust آن را پیکربندی، اجرا و پایش می‌کند؛ داده کاربر فقط درون Go می‌ماند (بدون IPC برای هر بایت). موتورهای فعلی دست‌نخورده می‌مانند. مجوز BAFT برابر AGPL-3.0 است و با BAFT-TUNNEL (اختصاصی) تفاوت دارد؛ مرز پردازه و ارائه سورس لازم است. هنوز هیچ کدی اجرا یا ادغام نشده است.

## 2. Real state of both repositories (retrieved 2026-11-08)

| Item | BAFT (`zarkmakerburg/BAFT`) | BAFT-TUNNEL (`zarkmakerburg/BAFT-TUNNEL`) |
|---|---|---|
| Main SHA | `2c8e14efdecabff392f061b2ea9538808597f2a2` [SRC] (git clone, depth 50) | `1080dbd3ef867cadcc05379a0f4b9fb87eecaefe` [SRC] |
| Latest change | Merge PR #304, v0.5.0 release notes | PR #53 `lab: check that the agent serves the panel and the wizard plan matches` |
| Version / release | CHANGELOG `0.5.0` dated 2026-11-02 [SRC]; GitHub release object not checked [NV] | Cargo `0.1.0`; pre-release `v0.1.0-phase1-pre` (cosign-signed per project memory) [INF] |
| Language / size | Go 1.27.3, about 67k non-test lines in `internal/` (including the BCC panel and ops tooling) [SRC] | Rust 2024, 6,736 lines in `src/` [SRC] |
| License | AGPL-3.0 (`LICENSE`, `COPYRIGHT`) [SRC] | `LicenseRef-Proprietary` (`Cargo.toml`) [SRC] |
| CI on this SHA | **[NV]** The BAFT API is not reachable from this session (read-only clone only). Repo docs record green Stage C, Step 5.7 and COR-01 runs on older SHAs (`STATUS.en.md`, SHA `29125392`, 2026-10-03). | `phase0` success on `1080dbd3` (run 42469105124) [CI]; two-host LAB E2E run 42469113232 on `1080dbd3` shows no conclusion yet [NV]; last success d05b727c (run 42466445718) [LAB] |
| Open PRs | not checked [NV] | #17 (draft, panel mockup) |
| Local working tree | Fresh clone, nothing to compare | Fresh clone on `claude/project-thread-c1etf1` from `origin/main`; no stash or other worktree exists in this container |
| Blocking facts | `STATUS.en.md` and `PLAN.en.md` are older than the code (dated 2026-10-03, code is v0.5.0). Treat them as history. | Real TCP packet loss untested, multi-hour soak not done, CRL distribution (ADR-0017) in progress, panel/wizard P4-001 in flight on the main thread |

Note: a stale remote ref existed in the container at start (origin/main showed an older SHA until fetched). All BAFT-TUNNEL facts here are from the fetched `1080dbd`.

Build and unit check (local, not CI): `go build ./cmd/baft` succeeds in 17 s and produces an 18.2 MB binary; `go test ./internal/protocol ./internal/routes ./internal/identity ./internal/resources ./internal/config` all pass. **[BUILD]**

## 3. BAFT architecture inventory

Line counts include test files (rough size indicator only). Verdict: **Use** = run as is inside the engine host, **Adapt** = needs a change or wrapper, **Skip** = out of scope for the engine, **Later** = candidate for rewrite.

| Package | Responsibility | Depends on | Tests | Verdict |
|---|---|---|---|---|
| `cmd/baft` (6.7k) | CLI: `run --file`, `config validate`, `tunnel`, `status`, `doctor`, `uninstall`, `recover`... | node, config, many ops pkgs | 19 test files | **Use** only `run` and `config validate`; the rest is BAFT's own ops tooling (menu, ingress, failover, bundle) and is not shipped in the engine host |
| `internal/node` (3.4k) | `Runtime`: listener and dialer roles, shards, carrier open/replace, recovery wiring, revocation reload, metrics | config, session, protocol, routes, identity, carrier, resources, telemetry, recovery | 9 | **Use** (the engine) |
| `internal/protocol` (1.3k) | BAFT/1 frame codec, HELLO, control, capability negotiation | none (leaf) | 9 incl. fuzz | **Use**; extractable leaf |
| `internal/session` (9.3k) | Session and Flow state machine, ACK/WINDOW, PADL scheduler, TWRL watermarks, ECRL recovery | protocol, resources, routes, scheduler, recordshape | 17 | **Use**; not separable from node in practice |
| `internal/carrier/h2` | HTTP/2 + mTLS carrier (also Noise handler) | identity | n/a | **Use** (MVP carrier) |
| `internal/carrier/ws`, `utlsdial` | WebSocket and uTLS carriers | identity | n/a | **Skip in MVP** (see section 5, engine order) |
| `internal/routes` (150) | Route table: fixed target, allowed peers, direction | none | 3 | **Use** |
| `internal/identity` (285) | mTLS config with exactly-one URI SAN allowlist, revocation set | std | 2 | **Use**; semantics mapped in section 12 |
| `internal/resources` (505) | Global receive and replay allocator, flow slots | none | 4 | **Use** |
| `internal/recovery` (1.4k) | Recovery durability helpers | session | 8 | **Use**; process-restart resume not implemented upstream [SRC STATUS] |
| `internal/scheduler` (384), `recordshape` (572) | DATA scheduling; record padding and shaping | session | 2+1 | **Use**; shaping only when both peers enable it |
| `internal/telemetry` (737), `metrics` (191), `redact` (256) | BCC telemetry exporter, Prometheus text, secret redaction | node | 2+1+1 | **Adapt**: telemetry stays disabled; metrics is the health surface |
| `internal/config` (2k) | Strict JSON/YAML config, validation | none | 11 incl. fuzz | **Use**; render target of the Rust adapter |
| `internal/securityinternal` (1k) | Security helpers | std | 5 | **Use** |
| `internal/bcc`, `agent`, `agentjob`, `cluster*`, `ingress*`, `publicingress`, `dnsfailover`, `failover`, `geoip`, `mesh`, `sshboot`, `sshmigrate`, `uninstall`, `release`, `tunnelnode`, `tunnelengine` (the large majority of the code) | BAFT's own control plane (BCC panel), agent, ingress, failover, installer | many | many | **Skip**. This is a competing control plane; BAFT-TUNNEL has its own agent, panel and installer. Not shipped in the engine host. |

Dependency graph of what is taken (arrows mean "imports"): `cmd/baft run` → `node` → {`session` → {`protocol`, `resources`, `routes`, `scheduler`, `recordshape`}, `carrier/h2` → `identity`, `config`, `recovery`, `metrics`}. Third-party: flynn/noise, utls, x/crypto, x/sys, modernc.org/sqlite (and its libc), yaml, brotli/compress (utls). **[SRC]** go.mod. Dependency audit and licence attribution are Phase B work.

## 4. BAFT runtime analysis (`internal/node.Runtime`)

All **[SRC]** at `2c8e14e`, `internal/node/node.go` unless noted.

- **Construction.** `NewRuntime()` (line 457); `Run(ctx, cfg)` (537) validates config, loads revocation (fail closed: a listener told to enforce revocation does not start without the file), checks key file permissions, builds the resource allocator and flow slots, optionally starts telemetry, starts the metrics HTTP server, then runs `runListener` or `runDialer` by `node.role`.
- **Listener.** `runListener` (738): TLS 1.3 `RequireAndVerifyClientCert`, one URI SAN, allowlist `server.allowed_peer_identities`; sessions per authenticated peer (shared "tenant" listener support exists, `tenants.go`).
- **Dialer.** `runDialer` (1059) with `dialerShard` (960): `transport.shards` parallel carriers, `transport.primary` and optional `fallback` carrier.
- **Carrier selection and switching.** `initialTransport` (1010), `recoveryTransport` (996): fallback carrier is used only by same-process recovery; logical Session identity and authority are unchanged.
- **Routes.** Outbound route = local listen plus remote route id; inbound route = fixed target plus allowed peers. The wire peer never chooses host/port (`docs/en/05-configuration.md`).
- **Identity and authorization.** mTLS plus URI SAN allowlist, per-route `allowed_peers`, revocation set (add-only; removal needs restart; established carriers of a newly revoked peer are cut on reload).
- **Recovery.** Same-process live-session carrier replacement, epoch fencing, bounded replay. Process-restart and reboot resume are **not implemented** (`STATUS.en.md`).
- **Shutdown.** Context cancel; `Run` waits for telemetry/probe writers before returning. SIGHUP reloads revocation and re-reads config (`reloadOnHUPFile`, `cmd/baft/main.go:181`).
- **CLI/config dependence.** `Runtime` takes a `config.Config` struct, so it can be driven by a generated file without the BAFT CLI menu or panel.
- **External control surface.** Config file, SIGHUP, loopback `management.metrics_listen` (Prometheus text, validated loopback and not the SSH port, `config.go:249-252`), process exit code. No control socket (`management.unix_socket` unused beyond a spool name). Test hooks on `Runtime` (`SetCarrierWriteFaultForTest` and others) are test-only and are not an API.
- **Key permission rule.** `requirePrivateKeyPermissions` rejects any key whose mode has group or other bits (`node.go:705`). BAFT-TUNNEL's LAB notes show systemd credentials arrive as 0400 or 0440; a 0440 file would be refused. Mitigation in section 13. **[SRC]**, mode on target systemd **[NV]** for BAFT.

## 5. Protocol analysis (BAFT/1)

Spec: `docs/protocol/baft1.en.md`, `docs/en/04-protocol-baft1.md`, golden vectors `docs/protocol/golden-vectors.json`. Frame header is 24 bytes big-endian, max payload 65536, max frame 65560; unknown types, non-zero flags/reserved and bad stream placement are errors (`internal/protocol/frame.go`). **[SRC]** Types: HELLO 0x01, HELLO_ACK 0x02, OPEN 0x10, OPEN_OK 0x11, OPEN_ERR 0x12, DATA 0x20, ACK 0x21, WINDOW 0x22, FIN 0x23, FIN_ACK 0x24, RESET 0x25, FIN_ACK_CONFIRM 0x26, RESUME_STATE 0x30, RESUME_DONE 0x31, READY 0x32, PING 0x40, PONG 0x41, GOAWAY 0x42, PROFILE_PROPOSE/ACCEPT/COMMIT 0x50-0x52, PADDING 0x60.

| Capability | Where | Evidence in repo docs | Known limit | Usable in BAFT-TUNNEL |
|---|---|---|---|---|
| Codec, bounds, fuzz | `internal/protocol` | local unit tests pass **[BUILD]**; protocol fuzz smoke green on `29125392` per STATUS **[CI]** (older SHA) | none noted | yes (inside Go) |
| HELLO/HELLO_ACK/READY, capabilities | `protocol/handshake.go`, `capabilities.go`, node | STATUS Stage B **[CI]** older SHA | v0.3.7 interop rules | yes |
| OPEN/DATA/ACK/WINDOW/FIN/RESET, flow control | `session` | COR-01: 1 GiB each way SHA-256 equal, 10.56 s on runner networking (STATUS) **[CI]** older SHA | local/runner network, not public-path throughput | yes |
| Backpressure, global allocator, TWRL, PADL | `resources`, `session`, `scheduler` | Stage C soak 25 repeats plus race (STATUS) **[CI]** older SHA | not a public benchmark | yes |
| Resume frames, ECRL, carrier switching | `session`, `recovery`, node | Step 5.7 recovery soak 5/5 on one SHA **[CI]** older SHA | same-process only; restart resume absent | partial: no restart resume |
| PROFILE_*, PADDING, record shaping | `recordshape` | benchmark log: 6.8 GB/s disabled, 3.3 GB/s padding-only, 15 MB/s with jitter (codec level, `reports/r3.1-codec-benchmark.log`) **[SRC]** | jitter costs throughput by design | later; off by default |
| Identity and route authorization | `identity`, `routes`, node | unit tests **[BUILD]**; Stage B **[CI]** | revocation add-only | yes, mapped in section 12 |
| Real Iran/abroad pair | BAFT `docs/roadmap/v0.5.0/REAL_HOSTS_LAB.md` | BAFT's own LAB, not BAFT-TUNNEL's **[NV]** here | not re-run | evidence only |

None of the CI/LAB rows above were re-run for this report. Anything marked "older SHA" must be re-run on the pinned BAFT commit in Phase B before the engine is called verified.

## 6. BAFT-TUNNEL architecture (from code)

- **Core.** Rust single binary `baft-tunnel` (`src/main.rs`): `serve`, `forward`, `apply`, `setup`, `agent`, config validation. Tokio, rustls, Yamux (`tokio-yamux`), snow for Noise. **[SRC]**
- **Engine abstraction.** `trait StreamEngine { id, accept, dial }` returning a `BoxedSecureStream` (`src/engine.rs`); implementations `TlsEngine` (`tcp-tls`) and `NoiseEngine` (`noise-tcp`). Authorization, hello, Yamux and stream limits sit above it in `src/tunnel.rs` and `src/authorization.rs`. `EngineKind` enum has two variants (`src/config.rs:22`). **[SRC]**
- **Lifecycle.** `serve` accepts sessions and applies pending-connection guards; `forward` supervises reconnect with exponential backoff (`next_backoff`, `supervise_forward`). There is **no engine fallback supervisor yet** (roadmap: after MVP). **[SRC]**
- **Identity.** TLS peers must present one `urn:baft-tunnel:node:<id>` URI SAN, SHA-256 pin, CRL check (`src/handshake.rs:243`); Noise uses enrolled static keys. Key custody via file or systemd credential (`src/key_custody.rs`, ADR-0013). **[SRC]**
- **Agent.** Loopback HTTP agent with bearer token, desired-state store, audit, panel and wizard plan routes (`src/agent.rs`). Desired state is validated and stored, **not applied** by the agent yet. **[SRC]**
- **Setup, installer, systemd.** `setup` CLI (P3-001 done), `scripts/install.sh`, `systemd/baft-tunnel@.service` (hardened: NoNewPrivileges, MemoryMax, LoadCredential). Ubuntu 22.04 / systemd 249 baseline (ADR-0016). **[SRC]**
- **ADR-0018** requires engines to keep fail-closed authentication, `engine`/`engine_fallbacks` config, imported code to pass licence check, secret scan, dependency audit, tests and "conversion to the trait". Item 4 confines Go to a sidecar behind a local IPC with the Rust core as sole authorizer.

Where BAFT enters: engine #3, after tcp-tls and noise-tcp, before ws-tls, uTLS and QUIC. Prerequisites: (a) a second engine kind ("managed engine") in the config model, (b) the setup/apply path able to render a third config shape, (c) the licence decision, (d) a packaged and pinned BAFT binary. BAFT's own `ws` and `utls` carriers overlap roadmap engines 4 and 5, so the MVP exposes only BAFT's `h2` carrier to keep the approved order.

## 7. Integration options

| Criterion | A: Go host + Rust supervisor (recommended) | A2: Go carrier-only sidecar, Rust mux | B: C ABI / cgo | C: extract Go packages | D: rewrite BAFT/1 in Rust | E: hybrid (A now, D selectively later) |
|---|---|---|---|---|---|---|
| Implementation complexity | Low to medium (config render, process supervision) | High (new Go IPC stream API plus Rust mux over it) | High (FFI, GC, threads) | Medium, but still Go | Very high | Low now |
| Security risk | Medium: authorization lives in Go from generated config | Low to medium: Rust authorizes each stream | Medium to high: shared address space, unsafe | Medium | Low if done well, new bugs likely | as A |
| Maintenance cost | Low: track upstream tag | Medium | High | Medium | High forever | Medium |
| Performance | Best: no payload IPC | Extra copy per byte over UDS | Good, but cgo call cost | n/a | Good | as A |
| IPC overhead | None for payload; control via files, signals, metrics | Per-byte Unix socket | Function calls | none | none | none |
| Extensibility | Per-engine process | Any Go carrier | poor | poor | high | high |
| Fit with current architecture | Needs a second engine kind | Fits `StreamEngine` shape best | Breaks "Go confined to a process" | n/a | Fits | as A |
| Existing code changed | New Rust module, config, installer | New Go package plus Rust client | Large | Large | Large | Small |
| Test and debug difficulty | Medium: two processes, clear boundary | Medium to high | High (segfaults, no clean logs) | Medium | High (need protocol parity) | Medium |
| Upgrade and rollback | Replace one binary; systemd unit per engine | Two moving parts | Single binary but coupled | n/a | Single binary | as A |

Why not B: ADR-0003/0018 confine Go to a process; cgo puts a garbage-collected runtime in the Rust address space, complicates `NoNewPrivileges`/seccomp, and gives no benefit if payload never crosses the boundary. Why not C: the useful value is the Session/Flow machinery, which is not separable from `node` (`session` imports `resources`, `routes`, `scheduler`, `recordshape`; node wires recovery); only `protocol` is a clean leaf and reusing a codec alone gives no engine. Why not D now: it repeats over 10k lines of recovery-sensitive logic with unfinished upstream semantics; keep as a candidate only for the leaf codec and the allocator, after BAFT/1 golden vectors are consumed by a Rust conformance test.

**Selected: A. Alternative: A2.** Choose A2 only if the owner decides Rust must authorize every stream; cost is new Go work and loss of BAFT recovery and flow-control semantics, because Rust Yamux would sit above the Go pipe.

## 8. Architecture of the recommended design

```
 baft-tunnel (Rust core)                         baft-engine (Go, AGPL, separate unit)
 ─────────────────────────                       ──────────────────────────────────────
 desired state ─► validate ─► render BAFT JSON ─► /run/baft-tunnel/<id>/baft.json (0600)
 identity/PKI/CRL ─► render certs, revocation ─► /run/baft-tunnel/<id>/{ca,cert,key,revocation}
 engine supervisor ── systemd start/stop/reload ─► baft run --file baft.json
        ▲  probes: process state, loopback metrics, route listener, byte-exact probe
        └──────────────────────────────────────── data plane: client ◄─TLS/H2─► peer, never via Rust
```

Boundaries: Rust owns desired state, PKI, config of record, unit lifecycle, health truth and the audit log. Go owns the carrier, Sessions, Flows, route listeners and per-stream enforcement of the rendered config. Nothing from Go is trusted for status without an independent probe (section 11).

Dual-engine conflict prevention: one systemd instance per tunnel id (`baft-tunnel@<id>` or `baft-engine@<id>`, mutually exclusive via `Conflicts=`), exclusive bind on the route listen port, and a per-tunnel lock file in `/run/baft-tunnel/`. BAFT has its own port-claim and instance-guard logic for its manager (`internal/tunnelnode/port_claim.go`, `instance_guard.go`), but those belong to its excluded control plane; the Rust side must not rely on them.

## 9. Adapter contract (v1, proposed)

Rust trait, separate from `StreamEngine` (a managed engine owns its data plane):

```text
trait ManagedEngine {
  fn id() -> "baft";  fn engine_version() -> (semver, source_sha, binary_sha256);
  fn capabilities() -> {carriers:["h2"], recovery:"same-process", shaping:false, udp:false};
  fn render(&RuntimeConfig, &Identity) -> RenderedBundle;     // pure, golden-tested
  fn validate(&RenderedBundle) -> Result;                     // runs `baft config validate`
  fn start/stop/reload(&Instance) -> Result;                  // via systemd, not exec from the agent
  fn health(&Instance) -> Health{process, listener, handshake, authorized, data_plane, observed_at};
}
```

- **Engine id/version.** `baft`, with the pinned upstream tag, source SHA and sha256 of the shipped binary recorded in the release manifest. Startup refuses a binary whose sha256 differs from the manifest.
- **Configuration authority.** BAFT-TUNNEL desired state is the only authority. The BAFT JSON is a derived artifact, never hand-edited, and carries a header marker (hash of desired state) so drift is detected by comparing hashes.
- **Rust → Go.** Rendered config, CA bundle, node cert, key path, revocation file, signals (SIGHUP reload, SIGTERM stop). Key never on argv or environment.
- **Go → Rust.** Exit code and stderr (redacted), loopback metrics, listener readiness (probe connect), and the result of an active byte-exact probe route.
- **Route management.** Routes are rendered 1:1 from tunnel config (`Serve` target is fixed, `Forward` listen is loopback unless `allow_non_loopback_listen`). Changing routes = re-render + validate + SIGHUP or restart, with the previous bundle kept for rollback.
- **Session lifecycle and statistics.** From metrics only: sessions, flows, bytes in/out, handshake errors, capability refusals (`baft_capability_refusals_total` is documented upstream). **[SRC docs]** exact metric names to be pinned in Phase B.
- **Recovery state.** Reported as "same-process only". After a Go restart all flows are new connections.
- **Restart policy.** systemd `Restart=on-failure`, `RestartSec=3s`, start limit as the existing unit. The Rust supervisor (when ADR-0018 item 2 lands) may switch to a fallback engine after capped backoff.
- **Failure behaviour.** Go engine stops → route ports close (no bypass), tunnel status goes `failed`, never `healthy`. Metrics endpoint unreachable while process alive → `degraded/unknown`, never healthy. Config render or validation error → zero mutation, previous bundle kept.
- **Control channel loss.** MVP has no live control channel, so "IPC cut" reduces to metrics probe loss; active sessions continue untouched and status turns stale/unknown.

## 10. Control-plane IPC (MVP and later)

| Stage | Mechanism | Notes |
|---|---|---|
| MVP | Files in a root-owned, `baft-tunnel`-group directory under `/run/baft-tunnel/<id>/` (0700 dir, 0600 files), systemd unit control, SIGHUP, loopback metrics, active probe | Needs no Go change. Metrics must bind loopback (enforced by BAFT validation). |
| Later (optional) | Go `baft engine-host` subcommand serving a versioned control API on a Unix socket created 0600 in the same directory, with `SO_PEERCRED` check of the peer uid, a monotonically increasing request counter (anti-replay), strict JSON with size bounds, and no key material in messages | Only if live reconfiguration without restart is needed. Requires Go code, which is a runtime change and out of scope now. |
| Future | FD passing or splice for zero-copy, only for A2 | Not needed in A. |

Backpressure and memory are already bounded inside BAFT (`limits.data_memory_mib`, receive and replay caps) and in the unit (`MemoryMax`, `TasksMax`). The engine unit gets the same hardening as `baft-tunnel@.service`.

## 11. Security boundary

Trust zones: (1) Rust core and its state directory; (2) Go engine process, unprivileged user, own state dir, no write access to Rust state; (3) network peers.

Rules:

1. IPC (files/signals/metrics) is local and root-or-service-user only; no TCP control port; metrics loopback only.
2. Private key reaches Go via a runtime file in a tmpfs directory, mode 0600, owned by the engine user, written by a privileged ExecStartPre step from the systemd credential (BAFT rejects group-readable keys, section 4). Never in argv, env or logs; redaction applied to captured stderr.
3. Route authorization cannot be bypassed by IPC failure, because there is no per-request IPC: authorization is data in the rendered config and enforced in Go on every handshake and OPEN. If config cannot be rendered or validated, the engine does not start.
4. Input validation at the only boundary (desired state → render): reuse `RuntimeConfig::validate`, identity charset rules and absolute-path checks; reject unknown fields on both sides (BAFT uses `DisallowUnknownFields`, `config.go:134`).
5. Replay: no replayable control messages exist in MVP (static files). Rendered bundles carry generation and desired-state hash; the supervisor rejects an older generation than the one applied.
6. **Narrowing of ADR-0018 clause 4.** With a whole-runtime host the Rust core cannot authorize individual streams. Compensations: the Rust side generates the only config; verifies the binary hash; runs **negative probes** in LAB and in an on-demand `engine verify` command (wrong identity refused, unauthorized route refused, revoked cert refused); and reports `authorized` status only from those probes. If the owner will not accept this narrowing, use A2.
7. A Go-engine fault must not be reported as healthy: health requires process up, listener accepting, a successful authenticated byte-exact probe in the last N seconds, else `unknown`/`failed`.
8. The Rust supervisor can detect the real security posture only indirectly (config hash, binary hash, probes). It cannot inspect Go memory. This limit is stated, not hidden.

## 12. Identity and authorization mapping

| Concept | BAFT-TUNNEL (Rust) | BAFT (Go) | Mapping |
|---|---|---|---|
| Node identity | cert with one `urn:baft-tunnel:node:<id>` URI SAN, SHA-256 pin, CA + CRL (`handshake.rs`), Ed25519 long-term identity per architecture doc | cert with exactly one URI SAN checked against `allowed_peer_identities` or `peer.allowed_identity` (`identity/tls.go:58`) | Same PKI can serve both: the Rust side renders the SAN string as BAFT's identity value. One stable identity per node per ADR-0012/0013. |
| Trust | CA file + pins | `tls.ca_file` | Same CA bundle file copied into the bundle |
| Authorization | tunnel id, direction, stream capability (`authorization.rs`) | route `allowed_peers`, fixed target, direction (`config.Route`) | Rendered from tunnel config; tunnel id appears as route id; direction maps to listener/dialer role plus route direction **[INF]** (Reverse mapping must be confirmed in Phase B) |
| Revocation | X.509 CRL, expiry checks, ADR-0017 distribution | `revocation.file` (identities, serials, fingerprints), add-only, restart to remove | Rust converts CRL entries to BAFT revocation list at every refresh and sends SIGHUP; a refresh that cannot be converted fails closed |
| Key/pin change | CA/pin rollover (ADR-0015, 0017) | restart with new files | Rollover = new bundle generation + restart |
| Cross-engine bypass | n/a | n/a | One engine instance per tunnel id; the engine cannot read other tunnels' bundles (separate dirs, distinct service users per instance **[INF]**) |

No engine may widen another's allowed peers: each tunnel's bundle contains only its own peers.

## 13. Operational security details to resolve in Phase B

- **Key permissions.** Prefer: systemd `LoadCredential=` file → `ExecStartPre=+` copies to `RuntimeDirectory` with mode 0600 owned by the engine user. Verify on Ubuntu 22.04 (systemd 249) where `/run/credentials` is not readable by pre-start commands (known from `baft-tunnel@.service` comments). Fallback: `ExecStart` wrapper in Rust (`baft-tunnel engine-launch`) that materializes the key then `exec`s the Go binary. **[NV]**
- **Telemetry.** BAFT's BCC telemetry must stay disabled in rendered configs. **[INF]**
- **Management socket.** Config requires `management.unix_socket` even though unused; render a path inside the instance runtime dir.
- **Logging.** `logging.payload=false` is mandatory in the renderer; stderr is captured with redaction.
- **Ports.** BAFT rejects the SSH management port on metrics (`config.go:252`); the renderer must reject it for routes too.

## 14. Performance assessment

No comparable data exists. BAFT numbers are local/runner or codec-level; BAFT-TUNNEL numbers are LAB integrity and soak, not throughput.

| Dimension | BAFT-TUNNEL baseline (tcp-tls, noise-tcp) | BAFT | Comparable now? |
|---|---|---|---|
| Throughput | not measured | COR-01 1 GiB/direction in about 10.6 s on runner networking [CI, older SHA] | No |
| Latency | application-level latency/jitter proxy: 64 KiB probes 0.38 to 2.09 s under injected impairment [LAB] | not measured | No |
| Multiplexing / concurrent flows | Yamux, `max_streams` default 8; yamux integrity test passes [CI] | up to `limits.max_flows`, 64-flow PADL test [CI, older SHA] | No |
| Backpressure | Yamux windows | TWRL/PADL, global allocator | design difference only |
| Recovery | reconnect with backoff, new streams only | same-process carrier replacement | BAFT advantage on carrier loss, not on process loss |
| Memory/CPU | forward RSS 5 MB, serve RSS 6 MB over 30 min [LAB] | not measured here; 18 MB binary; Go runtime plus SQLite linked [BUILD] | No |
| Packet loss | untested (real TCP loss open) | untested in BAFT-TUNNEL LAB | No |

Minimum benchmark (do not build more): on the two real LAB hosts, one run per engine (`tcp-tls`, `noise-tcp`, `baft`) with identical settings: (1) one 1 GiB byte-exact transfer each way, SHA-256 compared; (2) 32 concurrent 8 MiB flows, all byte-exact; (3) one carrier interruption of 10 s via a firewall rule on the isolated test port, time to resume; (4) RSS and fd sampled each 5 s on both hosts. Output: one table per engine. This reuses the existing two-host workflow (`.github/workflows/phase1-two-host-e2e.yml`) with an `engine=baft` input. Simulation results (netem containers) must be labelled separately.

## 15. License and ownership assessment (not legal advice)

- BAFT: AGPL-3.0 (`LICENSE`, `COPYRIGHT` "BAFT Project"), network clause 13 noted. Binary dependencies are listed in `THIRD_PARTY_LICENSES.md` (generated); direct modules include BSD-3 (Go std, x/crypto, uTLS, brotli, compress), MIT/BSD (flynn/noise, yaml, sqlite via modernc BSD-3). License text check for each is not repeated here. **[SRC]**
- BAFT-TUNNEL: `LicenseRef-Proprietary`, private/permissive-only `deny.toml`. **[SRC]**
- Contributors in BAFT history: owner identity (189 commits), Claude (95), `Amir m Alipour` (35), `mahan alipur` (6) **[SRC git log]**. Relicensing or dual-licensing BAFT code requires the owner to confirm those contributors' consent or a CLA; owning both repos does not remove their rights or AGPL's obligations for third-party code.
- Effect of shipping: distributing the Go binary triggers AGPL source-offer duties for it (and for modifications), and users who interact with a modified network service must be offered source (clause 13). Running it as a **separate process** with only config/signals/HTTP metrics between it and the Rust binary is the standard "arm's length" arrangement; static or in-process linking (cgo, option B) would weaken that argument.

Proposed policy (**[INF]**, owner decision): (1) keep BAFT's own AGPL licence unchanged for the engine host; (2) ship it as a separate binary in a separate package/unit with a visible source link and the exact corresponding source tag, plus its `THIRD_PARTY_LICENSES.md`; (3) BAFT-TUNNEL Rust stays proprietary and never links BAFT code; (4) if the owner wants a different licence for BAFT itself, first confirm contributor consent, then relicense as a deliberate act with its own ADR; (5) add a release-time licence gate that fails when the manifest lacks the source URL and licence texts.

## 16. LAB readiness

From `docs/LAB_AGENT_ACCESS.md`, `docs/REAL_HOSTS_LAB.md`, `docs/PHASE1_IMPLEMENTATION_STATUS.md` (no addresses or secrets reproduced):

- Two hosts (Iran, abroad), Ubuntu 22.04, systemd 249, in private GitHub Environment `baft-tunnel-lab`; SSH with pinned host keys and temporary runner key; workflow `phase1-two-host-e2e.yml` (manual) covers Direct and Reverse, listener restart, revocation, soak, latency/jitter proxy, `setup` flows. Standing owner authorization for isolated, cleaned-up LAB tests. **[LAB]**
- Not available/not proven: encrypted credential delivery on 22.04 (needs systemd 250+), real packet loss, multi-hour soak.
- BAFT on the hosts: needs only the static Go binary (no Go toolchain, no network fetch on hosts); copy by the existing workflow, run under a transient unit with the same isolation as current E2E, clean up after.
- Pairing: both nodes get certs from the same offline CA (`scripts/pki/baft-pki.sh`), the Rust renderer produces each side's config; success = authenticated session (metrics), then byte-exact probe both directions, then rogue identity refused, revoked cert refused, listener/engine restart recovery, interruption test, RSS/fd sampling.
- Do not change OS or systemd versions to make a test pass (project rule).
- Everything so far for BAFT in this lab is **[NV]**. The current LAB run 42469113232 has no recorded conclusion.

## 17. File-level implementation plan

Paths for existing files are verified. New files are proposals; no name is a claim that it exists.

| Path | Action | Responsibility | Depends on | Acceptance test |
|---|---|---|---|---|
| `docs/BAFT_ENGINE_INTEGRATION_DESIGN.md` | Create (this PR) | This document | none | review |
| `docs/ADR-0019-baft-engine-host.md` | Create (this PR) | Records method A, security narrowing, licence policy | ADR-0018 | review |
| `src/config.rs` | Modify | Add `EngineKind::Baft` ("baft") parsing; `validate` rejects it until `ManagedEngine` exists | existing enum at line 22 | existing config tests plus new reject test |
| `src/baft_engine.rs` (new) | Create | `ManagedEngine` trait impl: pure `render`, `validate`, pinned-version check | `config.rs`, `handshake.rs` identity rules, `key_custody.rs` | golden render test; invalid input renders nothing |
| `src/engine.rs` | Reuse | `StreamEngine` unchanged | none | existing tcp-tls and noise tests unchanged |
| `src/tunnel.rs`, `src/authorization.rs`, `src/handshake.rs`, `src/tls.rs`, `src/noise.rs` | Reuse, no change | Existing engines | none | existing tests and LAB E2E must stay green |
| `src/setup.rs` | Modify (Phase F) | `setup plan` knows engine `baft` and prints its files | `baft_engine.rs` | plan snapshot test |
| `src/main.rs` | Modify (Phase C/F) | `engine verify`, `engine-launch` subcommands | `baft_engine.rs` | CLI tests |
| `src/agent.rs` and `panel/` | **Do not touch now** | P4-001 is in flight on the main thread; add the engine choice only after it lands | n/a | n/a |
| `systemd/baft-engine@.service` (new) | Create | Hardened unit for the Go host; `Conflicts=baft-tunnel@%i.service` | `scripts/install.sh` | systemd-analyze verify plus LAB start/stop/restart |
| `scripts/install.sh` | Modify (Phase F) | Install pinned engine binary by sha256, create user and dirs, rollback | manifest | `scripts/test-install.sh` extended |
| `scripts/rollback.sh`, `scripts/uninstall.sh` | Modify (Phase F) | Engine-aware removal | same | existing tests extended |
| `release/engine-manifest.json` (new, location TBD) | Create | Pinned BAFT tag, source SHA, binary sha256, licence texts, source URL | release workflow | release gate fails if fields missing |
| `.github/workflows/release.yml` | Modify (Phase B) | Fetch signed BAFT artifact, verify hash, attach with licence files | manifest | dry run |
| `.github/workflows/phase1-rust.yml` | Modify (Phase C) | Step running pinned `baft config validate` on golden output | pinned binary | CI step |
| `.github/workflows/phase1-two-host-e2e.yml` | Modify (Phase E) | Add `engine=baft` input and negative probes | LAB | LAB run |
| `lab/scripts/*` | Modify (Phase E) | BAFT start/stop and probe helpers | LAB | LAB run |
| `deny.toml` | Reuse | Keeps Rust tree permissive-only; the Go binary is outside Cargo | none | `cargo deny` |
| `tests/`-style Rust tests (location TBD: current repo keeps tests inside `src/*.rs`) | Create | Render golden vectors, mapping tests | `baft_engine.rs` | `cargo test` |

Dependency graph for the implementer: `config.rs (enum)` → `baft_engine.rs (render/validate)` → `engine-launch/verify (main.rs)` → `baft-engine@.service + install.sh` → `agent/panel (after P4-001)`. The release manifest and licence gate are independent of Rust code and can proceed in parallel after Phase A.

## 18. Phased execution plan

**Phase A: architecture freeze.** Accept ADR-0019 (method, narrowing of clause 4, licence policy). Pin BAFT tag and SHA. Output: ADR merged. Risk: the owner rejects the narrowing, then switch to A2.

**Phase B: packaging.** Pinned `baft` binary build recipe or signed upstream artifact, sha256 manifest, third-party licence bundle, dependency audit (including the SQLite/libc stack linked for features BAFT-TUNNEL will not use), metrics name list, confirmation of Reverse mapping, key-permission method on 22.04. Acceptance: manifest and licence gate pass; BAFT CI/tests re-run on the pinned SHA and recorded. Risk: binary size/CGO-free build, upstream CI unreachable from here.

**Phase C: Rust adapter.** `EngineKind::Baft`, `baft_engine.rs`, validate gate, no process start yet, then unit start/stop through the supervisor path. Acceptance: golden render tests, existing engines' tests unchanged. Risk: two config models drifting; mitigate by generating only, never reading BAFT config back.

**Phase D: security integration.** Key materialization, CRL→revocation conversion with SIGHUP, route policy, redaction, binary hash check, `engine verify` negative probes. Acceptance: tests that a bad CRL conversion fails closed; secrets absent from logs.

**Phase E: integration LAB.** Pair on two real hosts, byte-exact both ways, Direct and Reverse, kill/restart, interruption, benchmark of section 14, resource sampling. Acceptance: all green on the exact SHA, run links recorded, cleanup verified. Risk: no real packet loss capability yet.

**Phase F: MVP integration.** `setup plan`, agent/panel engine choice (after P4-001), installer/upgrade/rollback, release packaging. Acceptance: a user chooses the engine and one port; `setup` needs at most the current command counts.

## 19. First executable commit and handoff

Valid baseline SHA: BAFT-TUNNEL `1080dbd3ef867cadcc05379a0f4b9fb87eecaefe`; BAFT `2c8e14efdecabff392f061b2ea9538808597f2a2` (re-check before starting). Proposed branch: `phase2/baft-engine-renderer`.

Order:
1. Merge ADR-0019 (owner delegation allows it).
2. Commit 1: `src/baft_engine.rs` pure renderer: input `RuntimeConfig` (Serve/Forward), output BAFT JSON with fixed fields (`schema_version`, `node`, `server`/`peer`, `tls` at runtime paths, `transport.primary="h2"`, `limits`, `recovery`, `routes`, `management` with loopback metrics and instance socket path, `telemetry` off, `logging.payload=false`); add `EngineKind::Baft` that `validate` still rejects with "engine not available"; golden tests for Serve, Forward, Direct, Reverse; negative tests (SSH port, non-loopback metrics, unknown fields). Nothing is started.
3. Commit 2: CI step running pinned `baft config validate --file` on the golden outputs.
4. Commit 3: CRL→BAFT revocation converter with tests.
5. Then packaging, unit, supervisor, LAB (Phases B, D, E).

Do not change: `StreamEngine`, `tcp-tls` and `noise-tcp` behaviour, authorization checks, the Ubuntu 22.04/systemd 249 baseline, the engine order of ADR-0018, any panel/agent file while P4-001 is open, and never import BAFT secrets or its BCC/agent/installer code.

## 20. Acceptance criteria

1. BAFT engine runs unmodified in the host with its core features. 2. BAFT-TUNNEL core stays Rust. 3. tcp-tls and noise-tcp tests and LAB E2E remain green. 4. Adapter implements the contract in section 9. 5. Identity/authorization cannot be bypassed; negative probes pass. 6. Real two-host byte-exact transfer both ways. 7. Health and lifecycle controllable, status truthful. 8. Restart and shutdown verified. 9. Failure of the control path never starts an engine or widens access. 10. Upgrade and rollback of the engine binary verified. 11. Real two-host LAB pass on the exact SHA. 12. Benchmark of section 14 recorded. 13. User picks the engine and a port; the rest is automatic. 14. Licence and dependency state documented in the release manifest.

## 21. Risks and blockers

| ID | Item | Label |
|---|---|---|
| R1 | Authorization delegated to Go (ADR-0018 clause 4 narrowed) | owner accepted by delegation, record in ADR-0019 |
| R2 | BAFT CI/release state on `2c8e14e` unknown from this session; docs older than code | [BLK] until BAFT API access or a local full test run |
| R3 | Process-restart resume absent: Go crash resets flows | known upstream limit |
| R4 | Key file mode 0440 rejected by BAFT | [SRC] risk, fix in Phase B |
| R5 | AGPL obligations and contributor consent | owner action before shipping |
| R6 | Two revocation models; add-only vs CRL removal | design item, conversion fails closed |
| R7 | Reverse-direction mapping unconfirmed | [INF] |
| R8 | Binary size/dependencies (SQLite, utls) for unused features | audit in Phase B |
| R9 | P4-001 touching `agent.rs`/`panel/` concurrently | sequence F after it |
| R10 | Real packet loss, multi-hour soak, ADR-0017 distribution still open for the product | existing gates |

## 22. Exact next actions

1. Merge this document and ADR-0019.
2. Phase B packaging task: pin BAFT tag/SHA, run BAFT's own tests on it, produce the manifest and licence bundle.
3. Start Commit 1 of section 19.
4. Ask the owner (one question only, not blocking the above): confirm contributor consent plan for BAFT relicensing, or confirm the AGPL separate-process policy as final.
