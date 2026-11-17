# Advisor assessment: BAFT-TUNNEL vs reference projects

Status: advisory document, 2026-11-17. **Not a decision record and not permission to change code.** Merge, release, deploy and host changes need their own authorization.
Related: [RAKHSH_INTEGRATION_ANALYSIS.md](RAKHSH_INTEGRATION_ANALYSIS.md) (not repeated here; this file only links to it), ADR-0010, 0019, 0028, 0030, 0031, 0038 to 0044, [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md) §9.

## 0. Evidence rules and limits of this review

- **BAFT-TUNNEL facts** come from files on `main` read on 2026-11-17 (roadmap, Rakhsh analysis, ADR-0044, `STABLE_0.2.2_EVIDENCE.md`). Source files under `src/` were **not** re-read for this document; where a status depends on code it is taken from those docs and marked "per docs".
- **Reference-project facts** come from each project's own `README.md` and `LICENSE` on its default branch, fetched 2026-11-17. That is **documentation, not verified behaviour**. Marketing text is treated as a claim, never as a fact. Nothing was executed, built or benchmarked. Pinned commit SHAs were not captured (API access to those repositories is out of this session's scope); the benchmark plan (§5) requires pinning them before any run.
- Evidence ladder stays: CI ≠ VM ≠ LAB ≠ Production. This file contains no performance, security or production-readiness claim about any project.
- Status vocabulary: Already Implemented, Partially Implemented, Designed Only, Missing, Experimental, Preview, Stable, Not Accepted.

### Licences of the reference projects (from each `LICENSE`)

| Project | Licence | Consequence for BAFT-TUNNEL (proprietary Rust core, ADR-0019 cl. 5) |
|---|---|---|
| Rakhsh | AGPL-3.0 | Ideas only. ADR-0038/0043: no code into core; copying allowed only into an isolated AGPL unit, nothing copied today |
| Hashem Panel | AGPL-3.0 | Same rule. Ideas only |
| Pangolin | Mixed: AGPL-3.0 files plus "Fossorial Commercial License" files, per-file headers | Ideas only; clean-room; dual licence makes any reuse a legal review item |
| FRP | Apache-2.0 | Code reuse is legally possible with notices, but it is Go (core stays Rust), so reuse would be a separate process or idea only |
| Rathole | Apache-2.0 | Rust, so code reuse is legally possible with notice and attribution. Recommend still studying design only; any copied file needs an ADR and a NOTICE entry |
| Tailscale | BSD-3-Clause | Go, ideas only in practice |
| Chisel | MIT | Go, ideas only in practice |

Not legal advice. Counsel review is advised before any decision to copy code or relicense.

## 1. Executive technical assessment

**Position.** Stable `v0.2.0` ships a Rust `tcp-tls` mTLS tunnel (Direct Stable, Reverse Preview), signed install, a local agent and panel, backup/restore/update with rollback, and a lot of real two-host LAB evidence per the roadmap. Its strengths against the field are evidence discipline (every claim is tied to a labelled run), mutual-TLS identity with CRL, a read-only/no-`CAP_NET_ADMIN` stance, and honest `Unknown` states. Compared with the references it is **narrower in transport and recovery** (one carrier per tunnel, no stream resume, no per-link health) and **stronger in security posture and truthfulness** than most.

**Where the product is thinnest relative to its own goal** ("reliable, recoverable, diagnosable"):

1. **Recovery is session-level only.** One yamux session on one TCP/TLS connection; when it dies every stream dies (reconnect backoff 1 s to 60 s per docs). The two features that fix this (multi-link ADR-0039, resume ADR-0040) are design plus pure unwired modules (#292, #293).
2. **Transport Intelligence is not wired to the runtime.** Throughput counters and an integrity probe exist; RTT, jitter, loss, per-link state exist only as a pure module (#293, unwired).
3. **The biggest live reliability signal is unexplained.** On the Iran path, sustained `tcp-tls` transfers were reset after about 0.5 to 1 MB on 2026-11-16 (runs 43897433228, 43910396414, 43945271865) while the same binary passed a 1 h soak earlier. Root cause unknown. This should outrank new features.
4. **Acceptance evidence is time-limited by policy.** Soaks are capped at 60 min and probe losses are excused by a human-judged "outage rule". That is honest but not reproducible by a third party.

**Verdict.** Do not chase the references' feature counts. The defensible niche is: *the tunnel whose health, recovery and integrity you can prove*. The shortest path there is to wire what is already designed (health, then a second link, then resume), prove it with a repeatable benchmark/fault harness, and resolve the Iran-path reset before claiming reliability.

## 2. Competitive feature matrix

Legend for reference columns: **D** = documented in README (claim, unverified), **–** = not found in README (not proof of absence), **n/a** = out of the project's scope. BAFT column = status per repo docs.

| Capability | BAFT-TUNNEL (status) | Rakhsh | Hashem Panel | FRP | Rathole | Pangolin | Tailscale | Chisel |
|---|---|---|---|---|---|---|---|---|
| Language / core | Rust core (fixed), Go only in separate BAFT engine | Rust (D) | panel wrapping FRP/Backhaul/Xray/GRE (D) | Go | Rust | not assessed | Go | Go |
| Direct tunnel | **Stable** (`tcp-tls`) | D | D (Iran→foreign) | D | n/a (reverse proxy) | n/a | n/a (mesh) | D |
| Reverse tunnel | **Preview** (`tcp-tls`; Noise Reverse Not Accepted) | D | D (FRP/Backhaul) | D (core model) | D (core model) | D (sites) | n/a | D |
| Multiple carriers | Missing at runtime (ADR-0010, 0039 designed; only `tcp-tls`, `noise-tcp`) | D (tcp, tls, ws, wss, quic, kcp, faketcp…) | D (six engines, GRE wraps) | D (tcp, kcp, quic, websocket) | D (tcp, optional Noise; more per README) | – | – | HTTP/WebSocket transport (D) |
| Multi-link / multi-lane | **Designed Only** (ADR-0039; pure `link_policy.rs` merged, unwired) | D ("lanes") | D (carrier swap, Autopilot) | – | – | – | – | – (multiple tunnels over one TCP, D) |
| Failover / failback | Designed Only (same) | D | D (auto-revert swap within 45 s, D) | – | – | – | n/a | reconnect with exponential backoff (D) |
| Stream resume | **Designed Only** (ADR-0040, needs protocol v3) | D (`resume`) | – | – | – | – | n/a | – |
| Reconnect / backoff | Partially: single-session, 1 s to 60 s doubling (per docs) | D | D | – | – | – | n/a | D (tunable exponential) |
| Health / telemetry | Partially: counters, `/v1/traffic`, integrity probe, status tiers; RTT/jitter/loss **Designed Only** (#293 unwired) | D (per-lane RTT/jitter/loss, per docs analysis) | D (per-link latency, alerts to webhook/Telegram) | D (dashboard, health checks) | – | D (audit logging) | not assessed | keepalive (D) |
| Identity & auth | **Already Implemented**: mTLS 1.3, pins, CRL, per-node PKI | D (per-node keypairs, Noise IKpsk2) | panel-level | token / TLS (D) | per-service token, optional Noise (D) | IdP + RBAC (D) | not assessed | SSH keys, users file (D) |
| Access control | Partially: authorization ADR-0011; no multi-user RBAC (core non-goal) | identity allow-lists (D) | panel | – | per-service token | **RBAC, per-resource (D)** | – | users config (D) |
| Privileged networking | **Not Accepted** (`CAP_NET_ADMIN` ban, ADR-0030) | raw/L3 modes (D, privileged) | GRE, spoofed pipe (D, privileged) | – | – | WireGuard-based | – | – |
| Plaintext mode | none | `encryption = "none"` exists (per prior analysis) | – | optional | – | – | – | – |
| UDP | **Missing** (#244 research) | D (kcp, quic) | D | D | D (UDP per README planning) | – | n/a | D ("TCP/UDP tunnel") |
| Multi-server / multi-tunnel | Partially: multiple tunnels per host (ADR-0036); Fleet **Missing** (Draft #171) | D (fleet, `--join` link) | **D (central panel, many nodes)** | – | – | D | D | – |
| Install UX | **Already Implemented**: signed one-liner, HTTPS panel, guided Wizard; pairing by bundle | D (one line + `--join rakhsh://…`) | D (one install both sides, setup bundle, path test) | binary | binary | docs/cloud | packages | binary / docker |
| Panel / API | **Already Implemented** local panel (fa/en), agent API | D (separate control panel + bot) | D (web panel) | D (dashboard) | HTTP API WIP (D) | D | admin console (service) | – |
| Integrity proof | **Already Implemented**: authenticated Integrity probe (ADR-0028) on protocol v2, Direct accepted | – | – | – | – | – | – | – |
| Benchmark evidence | Microbenches only (CodSpeed); no cross-project run | claims not assessed | – | – | **README benchmark section (claim, unverified)** | – | – | `test/bench/perf.md` (D) |
| Release / signing | **Already Implemented**: signed tags, SHA256SUMS, offline verify (ADR-0034) | CI/release workflows (D) | releases + CI (D) | – | releases (D) | docker/cloud | packages | releases, docker |

Reading the matrix: BAFT leads on **identity, signed distribution, integrity proof and truthfulness**; trails on **carrier breadth, multi-link, resume, fleet and measured per-link health**; refuses by decision the **privileged** modes that give Rakhsh and Hashem much of their censorship-evasion reach. That refusal is a product position, not a gap to close (see §7, R-9).

## 3. Architecture gap analysis

| Priority area (mandate §5) | Status | Gap and note |
|---|---|---|
| A. Transport Intelligence (RTT, jitter, loss, throughput, uptime, reconnects, handshake duration, queue depth, buffer usage, failure reason) | Throughput/bytes: Already Implemented (`/v1/traffic`, in-memory 1 h; 24 h persisted history is B5 with CI evidence only). RTT/jitter/loss/state: **Designed Only**, pure module #293 unwired. Uptime, reconnect count, handshake duration, queue depth, buffer usage, failure reason: **not verified in this review** | "Connected" is already not treated as "Ready" (receipt `ready` stays Unknown by design), which is the right base. Next step is wiring, not new design |
| B. Multi-carrier recovery | **Designed Only** (ADR-0039) plus pure `link_policy.rs` (#292, 16 tests, unwired) | Only one real carrier exists, so a link policy has nothing to choose between. See R-2: first prove it with two links of the **same** carrier |
| C. Stream resume | **Designed Only** (ADR-0040, strict protocol v3, no restart-resume) | Needs a harness that cuts the link mid-transfer and compares hashes before any claim. No code |
| D. User-space scheduling | **Designed Only** (ADR-0031, ADR-0044 Proposed) | ADR-0044 itself flags the risk that the relay hook is too far from the wire to help; ADR-0031 evidence gate allows "no gain". Good. Keep it behind measurements |
| E. Product experience | Install and Wizard: Already Implemented. Single-link pairing: Missing. Doctor: Designed Only (preflight is Partial) | See §6 |
| Head-of-line blocking | **Not covered by any ADR found** | One yamux session over one TCP connection: one stalled/reset TCP stream stalls every tunnel stream. Rakhsh "lanes" and FRP TCP-mux with multiple connections point at the alternative. Research item R-9 |
| Protocol v1 default | Partially: Integrity Unknown on v1, which is still `setup init` default | Roadmap O1 already covers; keep it P1 |
| Backpressure / buffers | Per docs "bounded yamux streams"; not independently verified here | Benchmark plan must measure RSS under slow-reader load |

## 4. Security and reliability review

Review of decisions and evidence, not a code audit.

**Strengths (per docs):** mutual TLS 1.3 with pins and CRL, per-node PKI, offline signature verification pinned root (ADR-0034, works when the Sigstore CDN is blocked), non-root hardened systemd units, `SO_PEERCRED` agent socket (ADR-0023), no firewall/route/sysctl mutation by default, backup/restore/rollback with tampered-restore refusal, independent assessment of P0+P1 surface with no Critical/High (2026-11-09), pre-Stable audit without Critical/High/Medium.

**Findings and risks:**

| # | Finding | Severity (advisory) | Evidence |
|---|---|---|---|
| S-1 | Iran-path `tcp-tls` resets after about 0.5 to 1 MB, root cause unknown (middlebox, time of day, flow length, fingerprint, or a BAFT defect are all still possible) | High for the reliability claim | roadmap §9.5, STABLE_0.2.2 evidence; 0 restarts, flat fd/RSS, automatic recovery |
| S-2 | Probe losses accepted by a human-judged "outage rule"; the control (a non-BAFT probe on the same path) is not mentioned as automatic | Medium (evidence quality) | STABLE_0.2.2 decision text |
| S-3 | Soak limited to 60 min by policy; mandate §7 asks for long-transfer tests | Medium (policy conflict, see §8) | roadmap §5 Phase 6 |
| S-4 | A resume buffer (ADR-0040) and a second accept path (ADR-0039) enlarge the attack surface (memory exhaustion, replay, session confusion) | Design-time, mitigations named in the Rakhsh analysis §9 | RAKHSH_INTEGRATION_ANALYSIS §9 |
| S-5 | Protocol v1 default leaves Integrity Unknown | Low (functional) | roadmap §9.1 |
| S-6 | Let's Encrypt HTTPS panel exposes a login on a public port; gate returns 404 without cookie per evidence | Low, accepted by design | STABLE_0.2.2 evidence |
| S-7 | AGPL BAFT engine runs as a separate process; engine hosting needs NOTICE/trademark obligations | Legal, tracked (ADR-0019) | project memory, ADR-0019 |

**Reliability matrix of what is proven** (LAB, per docs): Direct and Reverse restart/kill recovery in 2 to 4 s, 30 s blackhole recovery in 8 s (Reverse R3), 2% packet-loss runs, 1 h soaks. **Not proven:** recovery from path outages without session loss (impossible today), data integrity across a lost session (no resume), anything on Tailscale-like multi-path.

## 5. Performance benchmark plan (plan only, no results)

Goal: compare BAFT-TUNNEL (`tcp-tls`, Direct and Reverse), Rakhsh, Rathole and FRP on the same footing. **No benchmark claim exists today.** Existing CodSpeed benches are in-process microbenchmarks and are not cross-project evidence.

1. **Pin everything.** Record exact commit SHA or release tag and binary SHA-256 for every project, toolchain versions, build flags, crypto settings (Rakhsh Noise/AES-GCM, Rathole with and without Noise, FRP with TLS on, BAFT mTLS 1.3). Build from source on the same machine, no vendor binaries.
2. **Same hardware and OS.** Two hosts with identical CPU, kernel and Ubuntu 22.04 baseline; record CPU model, governor, NIC, MTU, `tc` settings in the report.
3. **Three environments, never mixed in one table:**
   - **E1 loopback** (single host, upper bound, CPU-bound);
   - **E2 controlled network** (two hosts or namespaces with `tc netem` set by the harness inside a disposable namespace; **no change to LAB host firewall/route/sysctl**; RTT, jitter, loss, bandwidth cap fixed and reported);
   - **E3 real path** (the two LAB hosts, Iran to abroad; time-of-day recorded; a non-tunnel TLS bulk transfer runs as control in the same window, as the research item in §9.5 of the roadmap requires).
4. **Metrics:** throughput (single stream, 64 parallel streams), connect and first-byte latency (p50/p95/p99), CPU (user+sys per GB), RSS and FD over time, handshake duration, reconnect time after fault.
5. **Workloads:** bulk (1 to 10 GiB), many small connections, mixed, idle keepalive. Each repeated at least 10 times; report median and spread, never a single run.
6. **Fault and recovery tests:** kill the link mid-transfer, 30 s blackhole, flapping link, restart of either end. After every fault compute SHA-256 of the received data against the source and record: connections lost, bytes retransmitted, time to resume, and whether data was **correct**, **truncated**, or **corrupt**. A project that drops the stream and requires an application retry is reported as such, not as a failure of integrity.
7. **Long transfer:** one run per environment E2 and E3 of at least several hours with periodic hash checks. This needs an owner exception to the 60-minute soak cap (see §8).
8. **Reporting rules:** raw logs and scripts published beside the table, environment labelled on every number, no cross-environment generalization, losing results published. A project's own README benchmark (for example Rathole's) is quoted only as "claimed by the project".
9. **Resources:** one benchmark owner, one LAB user at a time (existing LAB rule), cleanup as part of acceptance, no secret printed.

Acceptance of the plan itself: a harness repository path and a first E1-only dry run can be reviewed before any cross-project claim is written.

## 6. Product UX gap analysis

Goal: Install, Connect, Create Tunnel for a non-expert.

| Step | BAFT-TUNNEL today | Best reference pattern (per README) | Gap |
|---|---|---|---|
| Install | Signed one-liner, pinned to a tag, offline verify, `--domain`/`--ip` HTTPS, menu after install: Already Implemented | Rakhsh: one line that asks a few questions and installs the service | None material |
| Connect | Pairing bundles exchanged and verified; panel-managed enrollment; manual copy of a bundle | Rakhsh: second server runs `--join rakhsh://…` with nothing else to answer; Hashem: one setup bundle pasted on the other side | **No single join link/command for the second host.** Missing |
| Create tunnel | Guided Wizard, Direct; Reverse Preview; BAFT Preview Direct only | Hashem: path test tells which engines get through before you build | **No pre-flight path test across carriers**; Doctor Designed Only |
| Understand state | Honest Unknown/stale states, diagnostics tiers, integrity probe, no fake data | Hashem: one health model across every card, alerts to webhook/Telegram | Alerts partially (B5, CI only); no external notification channel |
| Operate | Backup/restore/update with rollback; update notice (ADR-0035) | Hashem: Watchdog, Autopilot, carrier swap that undoes itself in 45 s | Autopilot-style behaviour is **not accepted without evidence**; keep manual and receipted |
| Troubleshoot | Troubleshooting doc, diagnostics, no support bundle (O5 planned) | – | Support bundle |

Quick wins that fit the principles: a single `join` link (bundle inside, checksum, expiry) and a `doctor` that returns machine-readable PASS/FAIL/WARN/UNKNOWN, both already sketched in ADR-0042.

## 7. Prioritized technical recommendations

Format: Problem / Current State / Evidence / Options / Recommendation / Trade-offs / Security / Scope / Acceptance / Priority / Decision. "Decision" values are the advisor's proposal, not a decision.

### R-1 Wire the health model into the runtime and API
- **Problem:** Operators cannot tell a degraded path from a healthy one; "connected" is all the runtime knows.
- **Current State:** `src/transport_health.rs` merged, pure, unwired (#293); throughput and integrity evidence exist.
- **Evidence:** project memory and RAKHSH_INTEGRATION_ANALYSIS §10; Rakhsh, Hashem and FRP documents per-link/latency views (README claims).
- **Options:** (a) wire only RTT and state per session; (b) wire the full set; (c) wait for multi-link.
- **Recommendation:** (a) first, using the yamux/ping or the existing keepalive, exposing `rtt`, `jitter`, `loss_estimate`, `state`, `failure_reason`, `reconnect_count`, `uptime`, `handshake_ms` in the agent API and panel, every field with unit, interval, freshness; unknown stays Unknown.
- **Trade-offs:** small CPU/bandwidth for pings; metrics must not be read as internet-path claims.
- **Security:** read-only, authenticated agent API only; no new listener.
- **Scope:** `src/tunnel.rs`, agent API, panel health card, docs.
- **Acceptance:** unit tests for state transitions; LAB run that injects a blackhole and loss and shows the state change within a stated bound; fields absent when unmeasured.
- **Priority:** P0. **Decision:** Accept.

### R-2 Prove multi-link with two links of the same carrier before adding a new carrier
- **Problem:** A link policy with one link is untestable; each new carrier needs its own ADR and audit, which delays any recovery benefit.
- **Current State:** pure `link_policy.rs` merged (#292); only `tcp-tls`/`noise-tcp` carriers.
- **Evidence:** Rakhsh lanes and Hashem carrier swap are documented per README; ADR-0039.
- **Options:** (a) second link = same `tcp-tls` to a second address/port/IP; (b) wait for QUIC/WS carriers; (c) skip multi-link.
- **Recommendation:** (a): config accepts a link list, each link its own mTLS session and own health, failover and controlled failback with exponential backoff and a per-tunnel link cap.
- **Trade-offs:** two TCP paths may share the same bottleneck/middlebox; benefit is path diversity only when addresses differ.
- **Security:** same mTLS identity and authorization on every link (ADR-0011); bounded dials to avoid retry storms.
- **Scope:** config schema, `tunnel.rs`, API, panel link view.
- **Acceptance:** LAB: kill the primary path, traffic moves to the second within a bounded time, failback only after N healthy intervals, no flap loop, retry rate bounded; evidence label LAB, not production.
- **Priority:** P0. **Decision:** Accept (design), Research for failback thresholds.

### R-3 Stream resume, built after R-1 and R-2, harness first
- **Problem:** A session loss resets every stream; only the application can retry.
- **Current State:** Designed Only (ADR-0040, protocol v3).
- **Evidence:** Rakhsh documents offset/ACK resume (README claim); no other reference found with it.
- **Options:** (a) resume across lane change only (needs R-2); (b) also across restart (needs persistence, ADR-0040 says no); (c) do nothing, document.
- **Recommendation:** (a), after a fault-injection harness exists. Build the harness (R-5) before the feature.
- **Trade-offs:** memory per stream (window), complexity, protocol v3 break.
- **Security:** buffer caps, session id bound to handshake transcript, strict versioning (RAKHSH §9).
- **Scope:** protocol v3, relay, tests.
- **Acceptance:** mid-transfer link cut with hash-equal data on both ends, duplicates detected, window exhaustion bounded; no claim without this test.
- **Priority:** P1. **Decision:** Accept (design), implement after R-1/R-2.

### R-4 Resolve the Iran-path reset before claiming reliability
- **Problem:** Unexplained resets on sustained `tcp-tls` transfers.
- **Current State:** Research item in roadmap §9.5.
- **Evidence:** runs 43897433228, 43910396414, 43945271865 and forensics 43934069639, 43958046663.
- **Options:** (a) measurement matrix (time of day, flow length, TLS fingerprint, non-tunnel TLS control, other port); (b) change carrier behaviour speculatively; (c) accept and document.
- **Recommendation:** (a) only; make no workaround or evasion claim until measured. Automate the control probe so every soak logs it out-of-band.
- **Trade-offs:** LAB time, shared hosts.
- **Security:** no traffic-shaping tricks; stay user-space.
- **Scope:** LAB scripts, report.
- **Acceptance:** a report that states which factor (if any) correlates with resets, with the control data.
- **Priority:** P0. **Decision:** Research.

### R-5 Fault-injection and integrity harness (shared by R-3 and the benchmark)
- **Problem:** Recovery claims need cut/hash evidence; today each run is bespoke.
- **Current State:** LAB stages exist (kill, blackhole, loss); microbenches exist.
- **Options:** extend the LAB stage library vs a separate repo-local harness.
- **Recommendation:** extend; one script that runs a bulk transfer, injects named faults in a disposable namespace, and prints SHA-256 equality plus timing.
- **Acceptance:** reproducible on CI for loopback (E1) and on LAB for E3.
- **Scope/Security:** no change to host firewall; namespace-local only.
- **Priority:** P1. **Decision:** Accept.

### R-6 Make the soak/outage acceptance rule machine-checkable
- **Problem:** "Accept probe loss if journals prove an external outage" relies on human judgement.
- **Recommendation:** every soak also runs a non-BAFT probe (plain TLS and ICMP) between the same hosts and logs it; the gate excuses a loss only when that control also fails in the same window.
- **Acceptance:** gate script computes the verdict; historical runs re-classified for comparison.
- **Priority:** P1. **Decision:** Accept.

### R-7 One-command second-host join link
- **Problem:** Pairing is more manual than Rakhsh/Hashem patterns (README).
- **Recommendation:** an expiring, single-use join token that wraps the existing verified bundle (checksum, versioned schema), with the same authorization steps. No secret in logs.
- **Security:** token expiry, one use, bound to the pinning chain, no private key inside.
- **Acceptance:** two-host LAB run through the panel, negative cases (expired, reused, wrong checksum).
- **Priority:** P2. **Decision:** Accept.

### R-8 `doctor` and path test
- **Problem:** Users cannot tell which carriers or ports get through before building.
- **Current State:** Designed Only (ADR-0042); preflight partial. Hashem documents a path test.
- **Recommendation:** read-only `doctor` that returns PASS/FAIL/WARN/UNKNOWN and a carrier path test limited to dialing the peer; no config change, no privileged probe.
- **Acceptance:** machine-readable output, unit tests per check, LAB run.
- **Priority:** P1. **Decision:** Accept.

### R-9 Research: head-of-line blocking and session striping
- **Problem:** One yamux session over one TCP connection couples all streams.
- **Evidence:** Rakhsh lanes (README), FRP TCP-mux and pools (README); no BAFT ADR found.
- **Options:** keep one session; N sessions striped per tunnel; per-stream class pinning.
- **Recommendation:** measure first (R-5/benchmark) with 64 parallel streams plus one stalled stream; decide only on data.
- **Priority:** P2. **Decision:** Research.

### R-10 Privileged and plaintext modes stay out
- Raw/GRE/spoof/L3/`CAP_NET_ADMIN`, `encryption = none`, SSH-fleet and bot custody: **Not Accepted** (ADR-0030, ADR-0013/0014). Revisit only through an ADR; this assessment finds no reason to.
- **Priority:** n/a. **Decision:** Reject.

### R-11 Rathole-style per-service tokens
- BAFT mTLS identity already exceeds token-only auth. Per-service authorization is covered by ADR-0011. No action. **Decision:** Reject as duplicate.

### R-12 Notifications (webhook, optional bot)
- Hashem documents webhook/Telegram alerts. A webhook sink is low risk if secret-free and opt-in; a bot token adds custody concerns (ADR-0013/0014).
- **Priority:** P3. **Decision:** Defer.

## 8. Roadmap risk assessment

| # | Risk | Impact | Likelihood | Mitigation |
|---|---|---|---|---|
| RR-1 | Multi-link and resume stay unwired while roadmap spends cycles on 0.3.0 operability | Core differentiator slips | Medium | R-1/R-2 ahead of O-items that do not reduce outage impact |
| RR-2 | Second carrier needed before multi-link can be shown | Delay, new audit surface | Medium | R-2 same-carrier proof |
| RR-3 | Iran-path resets recur and are blamed on the product | Trust | High (observed) | R-4, R-6 |
| RR-4 | Single shared LAB pair, one run at a time | Throughput of acceptance work | High | batch runs, automate gate, queue discipline |
| RR-5 | Soak cap of 60 min vs mandate §7 long transfers | **Conflict, reported not overridden** | Certain | Owner decides: allow a one-off multi-hour benchmark window with LAB booked in advance |
| RR-6 | AGPL boundary: ADR-0043 permits copying into an isolated AGPL unit; mandate says no copying into core | No conflict found, because the isolated unit is not core; keep recorded | Low | Keep per-file provenance in the unit's NOTICE |
| RR-7 | Policy Pack (ADR-0031/0044) built before measurement | Wasted effort | Medium | ADR-0031 evidence gate already stops this; keep it |
| RR-8 | Fleet/control plane drift (draft #171, ADR numbering collision) | Plan confusion | Low | Renumber at revival |
| RR-9 | Owner-hands dependencies (S9, S10, real domain) | Blocked acceptance | Medium | Track as explicit blockers |
| RR-10 | `src/` code and ADRs diverge | Misleading status | Medium | This file's "per docs" items need verification before relying on them |

### Conflicts to report (not overridden)

1. **Soak length:** roadmap caps soaks at 60 minutes; mandate §7 asks for long transfers. Needs an owner exception for benchmark windows only.
2. **Roadmap ordering vs mandate priority:** roadmap §9.4 ranks operability (O1 to O7) for 0.3.0 and places the Policy Pack at 0.4.0; the mandate ranks Transport Intelligence and Multi-Carrier Recovery first. This document recommends R-1/R-2 move up alongside O1; not applied.
3. No conflict found with ADR-0019, 0038 to 0044 or the `CAP_NET_ADMIN` ban.

## 9. Open items for the owner

- Allow one multi-hour benchmark window (conflict 1).
- Confirm priority order R-1, R-2, R-4 before further 0.3.0 items.
- Confirm whether to capture pinned reference commits by granting read access to those repositories to a future session.

## 10. Provenance of ideas

Ideas studied (not code copied): Rakhsh lanes/resume/doctor/`--join` link (README and prior analysis), Hashem carrier swap with auto-revert and path test (README), Chisel exponential backoff and keepalive (README), FRP health checks and TCP multiplexing (README), Rathole per-service tokens and hot reload (README). No code from any reference project was read for this document or copied into the repository.
