# BAFT-TUNNEL — Product Roadmap and Verified Delivery Queue

**Status snapshot:** 2026-11-26 — **Stable `v0.3.1` is the latest released Stable.** The dated block in §9 is the current table; older text below is historical. **S5 remains OPEN** (no independent human sign-off).
**Source baseline:** `main` at `faba00fb3b12aac6395049c351be93027700890d` (PR #259, roadmap replan; code identical to `9b151ce`, PR #258). This SHA is the value when the file was written; `main` moves, so verify with `git rev-parse origin/main`. Evidence level for the release itself: signed Stable tag `v0.2.0` on `76c12cb` (= rc.12 binary, sha256 `4c922d02…9136`); post-release PRs #255/#257/#258 are merged to `main` and are **not** in any release yet. Earlier baselines (`b61765f`, `82d77b3`, U7 pinned to `1219f547`) are historical.
**Gate status:** P0-1..P0-5 remain closed. D2 panel-managed `tcp-tls` CSR/certificate lifecycle is implemented and merged in [PR #150](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/150) (merge `e5a7451f259be7df14187a58db5fe1c29c51a81b`); exact-head CI/two-host evidence is recorded in that PR, and a trusted repeat on the immutable RC.7 candidate passed in [LAB run 42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181). U1 CRL renewal and real two-host push are proven by [LAB run 42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521) on ancestor code SHA `8be74376176442106669f4a81293f0865629ecaf`: CRL number `0x1002→0x1003`, 30-day validity, both nodes report `crl_state=ok`, 2 MiB tunnel recovery, short/replayed CRLs refused. The repeat D2 LAB reported `host_drift=none`; the earlier unrelated nftables delta was not reproduced, but its source remains unattributed (S10).
**Release:** `v0.3.1` is the latest Stable release (not a prerelease), cut from reviewed `main` after the signed `v0.3.1-rc.2`; evidence level is hosted CI only and is recorded in [RELEASE.md](RELEASE.md). Earlier Stable `v0.3.0` and `v0.2.0` evidence stays in [STABLE_0.2.0_EVIDENCE.md](STABLE_0.2.0_EVIDENCE.md) and the release notes. Reverse and BAFT ship as **Preview**; `ready` stays Unknown by design. Production deploy is not claimed.
**Product status:** latest released version: **Stable v0.3.1**. Maturity: **Active Development** (Reverse, BAFT and stream resume are Preview). Production readiness: **not verified**; `ready` remains Unknown unless independently verified under the readiness contract. Main is protected; all code changes continue through PRs and exact-SHA CI.

**Next mission (2026-11-26): `v0.4.0`, the Adaptive Traffic Policy Pack.** `main` = `4cdb8c2` (merge of docs PR #451). `v0.3.1` is still the latest Stable and nothing product-facing has merged since its tag — the five commits after it are documentation only (#448, #449, #450, #451) — so `v0.3.2` would ship no product change and is not planned. For `v0.4.0` the order is: TP-0.1 research (#227, delivered as [POLICY_PACK_TP-0.1_RESEARCH.md](POLICY_PACK_TP-0.1_RESEARCH.md)); TP-0.2 accept ADR-0031 and fix the profile schema, classification and engine scope (#228); TP-2.0 engine-agnostic interface (#245, in `src/policy.rs`); TP-2.1 bounded queues and the deficit round-robin scheduler (#231, in `src/policy_sched.rs`); the ADR-0048 M1/M2/M3 experiments that choose how in-flight bytes are bounded (delivered 2026-11-26: the bound is the Yamux per-stream window, 256 KiB; M1/M2/M3 measured and rejected — [ADR-0048](ADR-0048-policy-queues-and-scheduler.md)); then the scheduler→relay connection, TP-2.2 fairness/overhead tests, static profiles (TP-3.x), adaptive control (TP-4.x) and API/CLI/panel/docs (TP-5.x). All of it is behind the `policy-pack` cargo feature, off by default. The one piece wired into the relay is the window bound, and only when `BAFT_TUNNEL_POLICY_PACK` is set; with the switch unset the data path is exactly today's. `v0.4.0` Stable is gated on the pack being wired and documented with per-tuple evidence labels; the two-host LAB is suspended, so measurement stays harness/CI (`simulated`) until an owner-authorized window. No profile is "ready" and no benefit is claimed. Fleet (#445) stays open on its two-host gates and is not part of 0.4.0.

This file is the single source of truth for phases, engines and delivery order. Item-by-item status, findings, risks, effort and owner-hands items behind the current queue: [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md). Detailed cross-project extraction, verified code state and implementation instructions: [CROSS_PROJECT_CAPABILITY_ALIGNMENT.md](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md). Exact historical test evidence and run IDs: [PHASE1_IMPLEMENTATION_STATUS.md](PHASE1_IMPLEMENTATION_STATUS.md). Persian task pointer: [PROJECT_BOARD.md](PROJECT_BOARD.md). HQ decisions and append-only evidence log: [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).

## 1. Product direction and scope

BAFT-TUNNEL is an independent Linux server-to-server connectivity product. The core stays Rust. The MVP focuses on installing, pairing, configuring, starting and verifying a secure tunnel between two Ubuntu 22.04 / systemd 249 hosts through a safe guided workflow.

Reference-project contribution:
- **BAFT:** session/flow, bounded resource usage, backpressure, recovery, identity and transport-engine lessons. The BAFT main engine remains a separate pinned Go process, not Rust source copied into this repository.
- **Hashem Panel:** later-stage ideas for multi-node inventory, orchestration, operational history, update and backup workflows.
- **BAFT BCC:** dashboard hierarchy, tunnel visibility, preflight/diagnostics, guided setup, responsive layout and Persian RTL/English LTR.
- These are patterns and requirements, not permission to copy code or expand the MVP into subscriptions/billing.

The current architecture explicitly does **not** define BAFT-TUNNEL as a customer/subscription panel. Third-party panels may integrate through a stable API later. Do not add customer billing, reseller tiers or account management to core MVP without an explicit product-boundary decision.

## 2. MVP target — v0.2.0

A non-expert operator can, on two Ubuntu 22.04/systemd 249 hosts:
1. Install a signed release.
2. Create a tunnel through the guided setup with only the essential choices.
3. Exchange and verify pairing bundles without manual editing of runtime configuration.
4. Start the selected engine through an authorized lifecycle path.
5. Prove an authenticated, authorized connection and bidirectional application-data integrity.
6. See truthful status, evidence freshness and an actionable failure explanation.
7. Recover from supported failures and remove only resources owned by this installation.

MVP does **not** require a central multi-user control plane, billing/subscriptions, QUIC/UDP, camouflage engines or automatic engine rotation.

## 3. Current verified phase state

Evidence classifications are deliberately separate: current source code, merged-PR implementation, local/headless tests, hosted CI and real two-host LAB are not interchangeable.

| Phase | Scope | Current state | Exit condition |
|---|---|---|---|
| 0 — Foundation | Repository, architecture, threat model, CI/LAB baseline and guardrails | **Done** for current two-host baseline | Historical foundation and workflows merged |
| 1 — Secure runtime | Rust TCP runtime, identity, authorization, PKI, installer, systemd, failure and adversarial tests | **U7 signed-RC.7 integrated two-host MVP acceptance passed** ([run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428)); U8 operator docs now merged in PR #165; bounded Direct scope only | Keep explicit scope limits and complete B2–B6 product-quality work (B1 merged) |
| 2 — Engine framework | StreamEngine, engine selection, Noise direct, BAFT process adapter and pinned Store | **Implemented in code; evidence gates differ by engine** | Noise stays Experimental; BAFT Store path got its own real-host proof before Wizard exposure (exposed as Preview, Direct only in PR #133) |
| 3 — CLI and local Agent | Setup plan/init/join, local API, desired-state validation, preflight, Apply/Verify/Receipt | **Local stages implemented** | Service lifecycle, authenticated peer/session and data-path verification accepted |
| 4 — Local panel | Overview, Servers, Tunnels, Wizard, Agent, Activity, Settings; fa/en and responsive behavior | **Installed lifecycle, diagnostics and operational workflow passed the trusted two-host U7 repeat; remaining product-quality gaps are B2–B6** | UI status and actions match a fully verified backend; no placeholder inventory/metrics |
| 5 — Additional transports | WebSocket/TLS, uTLS sidecar, QUIC/UDP, Relay and auto-rotation | **Future work, not MVP** | Each candidate has a real use case, ADR, security/licence review and acceptance matrix |
| 6 — Hardening and stable | Bounded reliability (maximum 60-minute soak), backup/restore, update lifecycle, fuzz depth, independent security review (P0+P1 surface assessed 2026-11-09; full pre-stable review paused by owner on 2026-11-11 and **re-authorized by the owner on 2026-11-24**, scope now the whole of current `main`) and signed stable release | **Open; S5 re-activated 2026-11-24; U7 included one 3600-second Direct soak; no multi-hour soak is planned** | All release gates closed and required independent review explicitly re-authorized/completed |

### Engine capability matrix

| Engine | Direct | Reverse | Product exposure |
|---|---|---|---|
| `tcp-tls` | Supported; real two-host integrity/recovery evidence exists; two-host Direct ops acceptance passed ([LAB run 42635190485](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42635190485), main `0b299d0`) | Real data path; dedicated LAB pass (run 42506805986) plus R3 breadth LAB pass (run 42525790470, P0-4) | Direct supported; Reverse **Preview** |
| `noise-tcp` | Experimental; Direct two-host evidence exists | Unproven; earlier label-only result is not valid data-path evidence | Direct Experimental; Reverse unavailable |
| `baft` | Separate-process adapter and launcher; Store→Launcher path, store rollback and signed release asset passed real-host LAB (P0-3) | BAFT's own behavior is separate and must not inherit Rust Reverse claims | Independent acceptance passed 2026-11-09 ([LAB run 42552860541](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42552860541), main `3a95c7f`): may be shown as **Preview, Direct only** (see [BAFT_ENGINE_ACCEPTANCE.md](BAFT_ENGINE_ACCEPTANCE.md)). Listed that way in `/v1/engines` and the Wizard since PR #133 (`c7dbd15`); guided setup (B4, ADR-0029) accepted on real two-host LAB [LAB run 43644908151](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43644908151), main `86ba404` |
| `ws-tls`, `utls-sidecar`, `quic` / UDP | Not implemented/advertised | Not implemented/advertised | Hidden / future |

**Evidence caveats**
- TCP-TLS Reverse Preview: [LAB run 42506805986](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42506805986), commit `2d8cd5e`. This is a bounded two-host pass, not soak, packet-loss or production evidence.
- TCP-TLS Reverse R3 (P0-4): [LAB run 42525790470](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42525790470), commit `5a72a25`: 5 service SIGKILL cycles (max recovery 3 s), public SIGKILL (3 s), 30 s blackhole (8 s), mid-transfer kill (2 s, user connection completed), 2% real packet loss, 1200 s soak with flat fd/RSS, busy port / wrong identity / wrong pin / revoked refused, no stray processes or nft rules. Reverse stays **Preview**; no Stable promotion is claimed. This is not production evidence.
- TCP-TLS Reverse independent acceptance (2026-11-09): [LAB run 42619848756](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42619848756), main `8fde7db`: clean install via setup + Apply, hardened non-root units, 1800 s soak with 0 failed probes, reconnect, backup/restore, guarded update + rollback, revocation, uninstall; `secure_channel stage failed` 0/0. May be shown as **Preview**, never Ready/Stable. See [REVERSE_ACCEPTANCE.md](REVERSE_ACCEPTANCE.md).
- TCP-TLS Direct two-host ops acceptance (2026-11-09): [LAB run 42635190485](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42635190485), main `0b299d0` (local build, binary sha256 `8fbbf657…dc5b`), `LAB_OPS=120`: clean install, guided setup `--direction direct` + Apply start on both nodes, hardened non-root units, 64 KiB/2 MiB integrity, diagnostics/status/traffic/audit on both sides, 120 s soak (9 rounds, 0 failed probes, fd/RSS flat, 0 restarts), serve/forward SIGKILL and Apply restart recovery (3–4 s), **backup/simulated loss/restore** (owners and modes identical, recovered in 4 s), **guarded update + automatic rollback** of a failing binary (4 s each), revoked forward cert refused, uninstall with listeners/units unchanged and 0 BAFT firewall rules. Not claimed: receipt `ready` stays `unknown` by design (never Ready); update was same code with a new digest (`cross_version_update=0`); soak is 120 s, not long-duration; unrelated firewall drift observed on the abroad host (not caused by BAFT-TUNNEL rules, but "no firewall effects" is not claimed). No tag/release/deploy.
- BAFT engine process under hardened systemd 249: [LAB run 42504156305](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42504156305), commit `2d8cd5e`. This predates PR #85's Store→Launcher change. The Store path was then proven separately (next bullet).
- Backup/Restore and Update/Rollback (ADR-0025, PR #111, main `8e4f797`): CI on PR head `6e029ee`, `ubuntu-2204-baseline` [run 42548499998](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42548499998) on real Ubuntu 22.04 / systemd 249: guarded update, crashing build rolled back automatically (old binary serving again, config/keys digest unchanged), tampered restore refused without stopping the service, destroyed config/PKI/key restored and service back, no private key material in logs or journal; `lab-config` run 42548500098 (staged tests). Hosted-runner evidence only; no two-host LAB run, no stable tag/release/deploy. Limits: health check proves the process stays up, not data flow; rollback restores binary and unit only.
- BAFT Engine Store → Launcher (P0-3): [run 42510577167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42510577167) on `2d2718b` (local-build engine; store launch, kill recovery, wrong pin refused) and [run 42522105698](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42522105698) on `06c273e` with the **signed `v0.1.0-rc.2` release asset** (cosign + SHA256SUMS verified, digest = `engine-manifest.json` pin `e4f04837…3647`): store install/verify, hardened systemd 249 launch, 2 MiB data, listener and engine-kill recovery, wrong pin refused, release B over A, pin follows `current`, rollback to A, tampered previous refused, A carries 2 MiB after rollback, cleanup OK. LAB gate only; BAFT stayed hidden from `/v1/engines` and Wizard until a separate exposure gate (passed: [BAFT_ENGINE_ACCEPTANCE.md](BAFT_ENGINE_ACCEPTANCE.md); exposed as Preview, Direct only in PR #133).
- Soak on signed prerelease rc.5 (2026-11-10), all PASS: A [run 42753138827](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42753138827) (Direct ops, 1 h), B [run 42761721111](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42761721111) (Reverse, 1 h), C [run 42770520604](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42770520604) (Direct, 1 h, 2% real TCP loss). Evidence: HQ Issue #2 comment 6762256022. Each run is 1 h; this is not a multi-hour soak.
- Panel pairing (U4/U5b, ADR-0026, 2026-11-10): [LAB run 42779915532](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42779915532), main `4887a72` (local build, binary sha256 `a27797b9…eb05e`), `panel_pairing=1`, `noise-tcp` Direct: clean install with `--register`, both agents in setup mode, preflight via the root setup helper, init/export/inspect/join on both nodes through the agent API only (no root setup command), wrong checksum flagged and its join refused, bundle free of key material, agents reloaded by themselves, Start via the agent, 64 KiB/2 MiB integrity, audit and helper receipts present, no token in agent state, uninstall with no host drift and 0 BAFT firewall rules. D2 was subsequently implemented and merged in PR #150; trusted two-host lifecycle repeat passed in LAB run 42996224181. This earlier run remains the evidence for the original bundle-pairing stage.
- Direct real 2% TCP-loss, 3600-second soak, CA/pin rollover and earlier setup/agent/panel runs are in [PHASE1_IMPLEMENTATION_STATUS.md](PHASE1_IMPLEMENTATION_STATUS.md). Their exact historical SHAs and scope remain part of the evidence; do not silently relabel them as new tests on current HEAD.
- PR #86 added local Apply init/join and panel receipt flow; its PR description reports local tests/browser QA and a temporary live-agent receipt, but no real-host LAB for that Apply flow.
- PR #88 is merged (2026-11-08). Its own description reported fake-controller tests only; the real-systemd 249 gate was closed afterwards (P0-1, run 42523536944, SHA 22e94f1).

## 4. Implementation baseline (historical assessment at main 0a7576b; later changes noted above)

### Implemented in current main
- Since `92b0645`: P1 operations evidence (diagnostics tiers, end-to-end `tunnel` state, traffic counters, audit v2; ADR-0024), local backup/restore and guarded update with automatic rollback (ADR-0025), security fixes SA-01..SA-06, SA-08, SA-10, and panel views of these (PR #120).
- Rust data plane with TLS 1.3 mutual authentication, peer identity validation, SHA-256 pinning, CRL-aware certificate checks, bounded Yamux streams and reconnect behavior.
- `tcp-tls` and experimental `noise-tcp` Direct engine paths behind the engine configuration.
- Actual `tcp-tls` Reverse mode, surfaced as Preview through capability contract, Preflight and Wizard. Noise Reverse remains blocked.
- BAFT main engine as a supervised external process with pinned binary configuration. PR #85 also wires the hash-checked Engine Store's `current` binary into the launcher.
- Local CLI and local authenticated Agent API.
- Preflight, setup planner, local desired-state validation and audit.
- Apply `init` / `join` with server-side checks, explicit confirmation, idempotency, file verification, safe rollback of only files created by the operation and secret-free receipt.
- Dependency-free static local panel served by the agent, with Persian RTL and English LTR, desktop/mobile navigation and honest empty/unknown/offline states.

### Gaps identified by the 2026-11-09 assessment (historical; see the verified status correction above)
_Status marks added at `main` `0a7576b`; this list is not a current-state snapshot. Details and corrections in [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md) §3._
- Apply `start` (PR #88 helper) is merged and LAB-proven on two hosts (P0-5), and since U3 (PR #137) Start enables it at boot, Stop disables it, Restart starts a new process; one real reboot of both LAB hosts brought agent and tunnel back by themselves (LAB 42692143748 (main `ff868a7`)). Apply never claims `ready`; receipt `ready` stays Unknown by design.
- ~~No end-to-end authenticated peer/session/data-path readiness result in the current status API~~ **Done** (ADR-0024, PR #109): diagnostics tiers, `tunnel` state and `last_verified_unix`; local listener acceptance is still not tunnel health.
- No central online enrollment/control plane or enrolled multi-node inventory.
- The Servers page is not a Hashem-style Fleet; it has local/configured-peer information only.
- ~~No live throughput counters exposed in the API~~ **Done** (PR #109 `/v1/traffic`, panel PR #120); history is in memory for one hour only.
- No read-only firewall inspection and no firewall mutation.
- ~~Audit actor/target completeness~~ **Done** (audit v2, PR #109). Structured Preflight/Diagnostics evidence codes + params and fa/en localization are **merged into `main`** (`311b706`), including the duplicate `diag.next.*` key fix and its regression assertion (`62d7ee6`); earlier exact-head CI evidence on `cc37f48` / `ec730c3` in PR #168 remains historical.
- Since U2 (PR #137, LAB 42692143748) the agent is a systemd service under its own user and setup sets owners itself. U1 CRL renewal/push is accepted (LAB 42656507521); U6 signed packaging and quick installer are complete in prerelease `v0.1.0-rc.7`.
- No customer/subscription/reseller/billing system in core.
- No production release. Independent security assessment of the P0+P1 surface done 2026-11-09 (`docs/SECURITY_ASSESSMENT.md`): 3 Medium and 3 Low fixed (#112, #114, main `f3c8b54`), 4 Low/Info open or accepted; no Critical/High found. This is not the pre-stable review of the full product.

### Panel implementation truth

The current MVP panel source is plain static HTML/CSS/JavaScript in `panel/`, served by the agent with no framework/build chain. Some high-level architecture/README text still describes React/TypeScript/Vite; reconcile those descriptions with the shipped source rather than assuming a React migration has happened. Do not add a frontend migration to the P0 queue.

## 5. Prioritized delivery queue

> **Superseded 2026-11-16:** the working order is now in [§9 Replan after Stable 0.2.0](#9-replan-after-stable-020-2026-11-16). The table below is the MVP queue as it stood before the release and is kept for evidence.

### Historical queue: usable MVP first (verified refresh 2026-11-12, `main` `32bf85f`)

The P0 gates proved the runtime; they did not make the product usable by a non-expert operator. The queue below replaces "P1/P2 held" as the working order. IDs, evidence, effort, dependencies, decisions and owner-hands items are in [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md) §4–§6. Scope rule: P1 acceptance and panel integration only; no new engines or protocols.

| Order | ID | Work | LAB | Owner hands |
|---|---|---|---|---|
| now | Q2 | Complete the remaining documentation truth pass and refresh HQ board text; preserve historical evidence and do not delete branches without a verified merged-PR mapping | — | — |
| done | Q1 | `phase1-rust` runs on push to `main` and weekly; exact-main checks are active | CI | — |
| done | Q3 | Preflight `directory_writable` blocks Apply when the agent cannot write; bilingual remediation is present | CI + unit | — |
| done | Q4 | BAFT remains visible as Preview but is disabled in Wizard until guided configuration exists | browser QA + capability tests | — |
| done | U1 | 30-day `renew-crl`, renewal runbook, CRL status and push helper; real two-host push/reconnect/rejection evidence in [run 42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521) | REAL-LAB | — |
| done | U2, U3, U5a | Agent systemd service, boot persistence and lifecycle controls (PR #137 and follow-ups; LAB 42692143748) | REAL-LAB | — |
| done | U4, U5b | Panel pairing, root config promotion, bundle import/export and D2 CSR/certificate enrollment (PR #150; trusted repeat LAB 42996224181) | REAL-LAB | — |
| done | U6 | Signed prerelease packaging and quick installer; `v0.1.0-rc.7` published | release workflow | — |
| done | U7 | Signed RC.7 MVP acceptance v2: clean install, guided Direct setup, lifecycle, diagnostics, 64 KiB + 2 MiB hash integrity, backup/restore, RC.5→RC.7 upgrade, failed-update rollback, revocation, uninstall and cleanup | REAL-LAB PASS [43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428); 3600s soak, 264 rounds, 0 failed probes | — |
| done | U8 | Final operator quickstarts en/fa, installed Agent/menu boundary, panel-managed tcp-tls PKI, Direct Wizard, start/diagnostics, CRL, backup/update/rollback, uninstall and tier troubleshooting | [PR #165](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/165) merged at `52a2825`; exact-head CI and relative links PASS | — |
| done | B1 | Structured Preflight/Diagnostics evidence codes + typed params; localized fa/en evidence, sources, remediation and next actions | Merged into `main`: `311b706` (localized structured evidence) and `62d7ee6` (duplicate `diag.next.*` key removal plus a regression assertion); historical exact-head CI evidence in [PR #168](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/168) | — |
| done | B2 | Authenticated in-product integrity probe ("Test tunnel") under [ADR-0028](ADR-0028-authenticated-integrity-probe.md) | **Direct accepted**: trusted two-host run [43407066796](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43407066796) on fixed candidate `b15b0a3f49e0e5b7c4fcaaec3b3da49cd22514c6`; binary SHA-256 `726e292afe323becd9cd6c4d79c7fb0cd4ae69013334369e304176b248c69e89`. Protocol-v2 setup, authenticated Direct sessions, 64 KiB/2,097,289-byte data paths, both in-product probes, audit/request hashes, restart persistence, cooldown, final data path and cleanup passed. Negative path: controlled in-memory corruption relay test in PR #193 rejects with SHA-256 mismatch. Direct only; Reverse remains separately gated. No live two-host tamper injection is claimed. | REAL-LAB + CI PASS |
| done | B4 | BAFT guided setup, Direct only (ADR-0019 phase F): `setup plan/init/join`, Engine Store in `install.sh`, Apply start/stop, selectable Wizard | [PR #201](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/201) merged (`86ba404`); REAL-LAB via Wizard/agent API [43644908151](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43644908151) with signed rc.7 engine. BAFT stays Preview, Direct only; status/traffic Unknown | — |
| next | B3 | Read-only firewall visibility. **Constraint:** the owner rejects a `CAP_NET_ADMIN`-based design; firewall stays `Unknown` until a safe design is accepted (draft ADR [#174](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/174); capability probe [#196](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/196)) | CI probe, then read-only LAB | — |
| next | B5, B6 | B5 history/alerts; B6 accessibility audit + phone check | as listed | O3 for B6 |
| Phase 6 | S1–S10 | Stable hardening; no soak longer than 1 hour is planned. Keep external nftables attribution as a separate open environmental item (S10) | ≤60 min where soak is needed | owner review re-authorized 2026-11-24 (S5 active) |

Not before the MVP queue is done: P1-1 enrollment and P2 Fleet/Jobs (Tier L in the plan).

Two merged streams are outside this queue and carry no product claim: CodSpeed performance benchmarks for the tunnel hot paths (`.github/workflows/codspeed.yml`, `benches/tunnel.rs`, `7bf0b9a`) and the Central Fleet design system (`DESIGN.md`, `0e66077`). The Fleet material is design documentation only; Fleet/control-plane implementation stays in Tier L and neither stream reorders a roadmap item.

### P0 — critical path (all five gates closed; criteria kept for history)

| ID | Work | Required result / exit evidence |
|---|---|---|
| P0-1 | PR #88 systemd control helper | **Real-systemd 249 gate closed 2026-11-08** (hosted runner, run 42523536944, SHA 22e94f1; two-host LAB proof rides on P0-5). Original criteria: Security review; real Ubuntu 22.04/systemd 249 LAB for start/stop, scope restriction, invalid/unregistered instance refusal, failure rollback, idempotency/stale requests, no unrelated state change and cleanup |
| P0-2 | Apply lifecycle and readiness truth | **LAB gate closed 2026-11-09**: staged receipt with authorized-session indicator (run 42524638625, SHA 231ba9d; hosted systemd 249) and the two-host proof in P0-5 run 42538458094 (main 92c4e70, release rc.4): `apply_start_serve/forward=applied`, `receipt_session=observed`, 64 KiB + 2 MiB `integrity_64k_2m=1` measured by the LAB stage, `receipt_ready=unknown`. Apply itself never claims data integrity or `ready`; it stays Unknown by design and is measured by Verify/LAB. Original criteria: Keep init/join receipt semantics; distinguish service active, listener accepting, authenticated peer, authorized session and bidirectional data integrity; unknown stays Unknown |
| P0-3 | BAFT Engine Store → Launcher | **LAB gate closed 2026-11-08** (runs 42510577167, 42522105698; signed rc.2 asset). Exposure in Wizard was a separate gate (passed; Preview, Direct only, PR #133) |
| P0-4 | TCP-TLS Reverse R3 | **LAB gate closed 2026-11-08** (run 42525790470, SHA 5a72a25; Reverse stays Preview pending owner review). Original criteria: Agent restart, repeat/reconnect, stale-session replacement, port conflict, longer soak, packet loss, failure interruption and cleanup; Reverse remains Preview until accepted |
| P0-5 | Integrated signed-artifact MVP LAB | **LAB gate closed 2026-11-09** (run 42538458094, main `92c4e70`, signed prerelease v0.1.0-rc.4 `c1652caa…f00f`; init/join through the guided CLI, start through Apply; unrelated host firewall drift observed on abroad, no BAFT-related firewall change). Original criteria: Clean two-host installation using signed release, supported setup path, accepted lifecycle, authenticated tunnel, 64 KiB + 2 MiB integrity, negative identity/pin/revocation cases, recovery, rollback/uninstall and cleanup |

**Execution order:** P0-1 → P0-2 → P0-3 → P0-4 → P0-5, unless an independent task can be done without sharing LAB hosts or conflicting files. Run only one real-host LAB workflow at a time.

### P1 — product operations and diagnostics

1. Authenticated Node enrollment and small remote Node inventory, only after a reviewed control-plane trust-boundary ADR. — _Not started; after the MVP queue (L1)._
2. Real authenticated peer/session/data-path diagnostic endpoint; TCP-only reachability must remain a lower evidence tier. — **Done** (ADR-0024, PR #109, LAB 42545941726).
3. End-to-end tunnel state and last-verified timestamp. — **Done** (PR #109).
4. Complete audit actor, target and resource identity. — **Done** for local Apply (PR #109); remote Job receipts wait for item 1.
5. Measured throughput/byte counters and history with units, interval, provenance and freshness. — **Done** (PR #109, panel PR #120); 24 h persisted history and availability/stale alerts are B5 (implemented, CI evidence only).
6. Machine-readable Preflight/Diagnostics evidence codes + params and localized explanations. — **Done**: merged in `311b706` with the duplicate-key fix `62d7ee6`: B1.
7. Read-only firewall visibility. — _Not started_: B3.
8. Safe “Add from Link” import with versioned schema, checksum, strict validation and preview. — _Not started_; delivered as the panel bundle import in U5b.

### P2 — multi-node operations

1. Fleet inventory, Node groups, version and health freshness.
2. Remote orchestration via scoped Jobs, idempotency, receipts and rollback.
3. Update/rollback and backup/restore; require restore acceptance, not just backup success. — **Done locally** (ADR-0025; restore accepted on LAB in runs 42635190485 and 42619848756, same version); cross-version update and remote Jobs open.
4. Monitoring, alerts and operational history.
5. Third-party integration API for external customer/subscription panels if the owner prioritizes it.

### 5a. Optional add-on packs (after Stable 0.2.0)

**Adaptive Traffic Policy Pack — Proposed, not started.** Optional add-on (one shared engine, profiles Balanced/Gaming/Trading/Streaming/Bulk, Traffic Policy Engine, Adaptive Optimization) that tunes BAFT-TUNNEL's own queueing, scheduling and fairness. Userspace only (no `CAP_NET_ADMIN`, ADR-0030); no claim about internet path latency; does not manage users or replace Xray/panels. Priority: after Stable 0.2.0 and Phase 6 gates; it must not delay core work or LAB use for them. Plan, phases, acceptance matrix and benchmark plan: [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md). Decision: [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) (Proposed). Epic [#226](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/226), work items #227-#242. Every phase is Proposed; no profile is ready without measured evidence against Balanced and Baseline. Per engine and carrier: no engine inherits another's profile claims, and Gaming/Trading benefit claims need the separate UDP/datagram carrier track ([#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244), own ADR, audit and LAB; Phase 5 transports). The policy interface is engine-agnostic ([#245](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/245)).

### P3 — new protocol/transport options

WebSocket/TLS, uTLS sidecar, QUIC/UDP, Relay, multi-engine failover and automatic rotation are gated research. Do not begin them while P0 remains open unless there is an explicit owner priority change and a written acceptance plan. Do not claim traffic invisibility or censorship resistance.

## 6. Fixed constraints

1. Rust remains the core language; Go does not enter the core.
2. BAFT stays a separate pinned process with its own lifecycle and licence/attribution obligations.
3. Hashem/BCC/BAFT lessons are patterns, not source-code transfer.
4. TCP-TLS Reverse remains Preview; Noise Reverse remains unavailable.
5. BAFT acceptance gates passed on 2026-11-09 for Preview/Direct only ([BAFT_ENGINE_ACCEPTANCE.md](BAFT_ENGINE_ACCEPTANCE.md)); PR #133 (`c7dbd15`) lists it as Preview, Direct only in `/v1/engines` and the Wizard, since B4 (PR #201, ADR-0029, [LAB run 43644908151](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43644908151)) the Wizard configures it, still Preview, Direct only. It is never Stable or Reverse without further evidence.
6. No fake success, stale-green UI, unsupported action button or unverified engine option.
7. Apply/Job operations must be authorized, scoped, idempotent where possible, verifiable, receipted and recoverable.
8. No firewall/routes/sysctl/production-host changes by default.
9. Use Ubuntu 22.04 / systemd 249 as the baseline; no OS/systemd upgrades as a shortcut.
10. One LAB run at a time; cleanup is part of acceptance.
11. No production release until explicit release gates and required owner authorization are satisfied.

## 7. Release gate checklist

Gate-by-gate status at `main` `0a7576b`: [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md) §8. The boxes below stay unticked until a release candidate is accepted against them.

- [ ] P0-1 through P0-5 accepted with exact SHA and CI/LAB evidence.
- [ ] Engine catalogue, CLI, Preflight, Wizard and runtime agree.
- [ ] Reverse Preview status and Noise Reverse unavailability remain truthful.
- [ ] BAFT Store path uses the pinned release artifact and its manifest; licence/attribution package verified.
- [ ] Apply receipts disclose exactly what happened and what remains unverified.
- [ ] Signed asset, checksum, signature, manifest and rollback verified.
- [ ] Upgrade/restore/uninstall tested on baseline hosts.
- [ ] No unresolved critical/high security or cleanup issue.
- [ ] Independent security review completed before a stable v1.0 claim (owner previously waived one for current development; waiver is not a review).
- [ ] HQ Issue #2, this Roadmap, relevant ADRs and implementation evidence updated.
- [ ] Release authorization treated separately from code merge.

## 8. Required HQ update format

Every meaningful milestone must append an entry to [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) with:
- operation/decision/test type and date;
- actor, scope and phase;
- exact branch and commit SHA;
- PR and exact CI/LAB run links;
- result: PASS / FAIL / UNKNOWN;
- measured facts and cleanup outcome;
- unresolved risks and next action.

The Persian [PROJECT_BOARD.md](PROJECT_BOARD.md) remains a short navigation pointer; it is not a second status database.


## 2026-11-24 S5 re-authorization addendum

The owner paused the external full security review on 2026-11-11 (Phase 6 and O6 in [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md)) and lifted that pause on 2026-11-24. S5 is active again and its scope is the **whole of current `main`** (`a3637aa1f2fb11b8cea8bba044b765362210cb48`), not only the PR #416 instance-isolation delta. The suspension history is kept, not deleted. The published S5 addendum remains a delta review written by a contributor to #416 and is not an independent human review; closing S5 still needs a reviewer outside the authors and implementers of the reviewed changes. Exact-SHA CI on `a3637aa` is green (10/10 check runs, [phase1-rust run 45278026658](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/45278026658)). The two-host LAB stays suspended by owner order. No deploy, tag, release or Production Acceptance is made here.


## 2026-11-11 U7 acceptance addendum

Trusted U7 [run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) completed all three jobs successfully on product parent SHA `1219f547e6d9c2fe9605385f37a7d5dd8dab2995`: release-tag ancestry checks, signed RC.7/RC.5 asset verification, and the real two-host U7 acceptance. The final marker was `PHASE1_TWO_HOST_E2E=PASS` and `OPS_RESULT` recorded `integrity_64k_2m=1`, `cross_version_update=1`, `backup_restore=1`, `bad_update_rolled_back=1`, `revoked_refused=1`, `uninstalled=1`, `soak_s=3600`, `rounds=264`, `failed_probes=0`, `restarts=0->0/0->0`, and `baft_fw_rules=0`. Diagnostics integrity and receipt ready remained Unknown by design. The run did not exercise CRL push (`crl_push=0`); U1 remains separately evidenced by run 42656507521. The abroad host showed six lines of unrelated address-set drift; its source is unknown and remains S10. No firewall neutrality, Reverse, Noise, BAFT, Stable or production claim is made.


## 2026-11-13 B2 Direct feature-acceptance addendum

Trusted run [43407066796](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43407066796) passed all three jobs on orchestration/main SHA `82d77b3113ba6d966d1792f772940c70c0dfb4bc`, using the fixed product candidate `b15b0a3f49e0e5b7c4fcaaec3b3da49cd22514c6`. The candidate binary SHA-256 was independently read from the uploaded artifact and verified as `726e292afe323becd9cd6c4d79c7fb0cd4ae69013334369e304176b248c69e89`.

The real two-host Direct run passed ordinary 65,536-byte and 2,097,289-byte data paths before and after the in-product probes; protocol-v2 Setup CLI selection; authenticated sessions on both peers; 64 KiB integrity probes in both directions; audit resource and ADR-0024 request-hash assertions; agent-restart persistence; rate-limit/cooldown persistence; and cleanup. Destination payload bytes remained unchanged. The reported 24 zero-payload TCP connections are attributable to the Agent's periodic listener/status health checks, not probe payload; PR #194 records this distinction.

Negative-path evidence is separate and deliberately scoped: PR #193's controlled in-memory relay flips one probe-payload byte while preserving framing/nonce; the production initiator rejects it with `SHA-256 mismatch`. The production probe implementation at the fixed candidate SHA is unchanged by that test-only PR. This satisfies ADR-0028's controlled test-harness corruption requirement; it is **not** a claim of live two-host tamper injection. The local authenticated v2 session integration test and the two-host positive path are separately covered. Apply `ready` remains Unknown by design.

B2 is accepted for **Direct only**. Reverse remains unavailable until its independent ADR-0021 implementation and acceptance gate passes. No release, tag, deployment, production-host mutation, or Phase 2 transition is implied. Failed run #13 remains preserved as a harness accounting failure and was corrected by PR #194 before run #14.

## 2026-11-25 current-state verification addendum

Live re-check on 2026-11-25: `main` is `62c0ba274c8d2843ece1ee9030feb351c5444367` (merge of PR #440), its exact-SHA checks are 10/10 `SUCCESS`, the latest Stable tag is still `v0.3.0`, and `Cargo.toml` reads 0.3.1 with `v0.3.1-rc.1` as a prerelease. S5 is **OPEN** (no independent human sign-off; green CI is not a substitute) and Reverse stays **Preview** with the R9 two-host LAB run outstanding under the owner's suspension.

**2026-11-26 release train and PR sweep.** `main` = `4de83dd35234eaca135873aa6c9d76efba02e9b7` (squash merge of PR #446). Merged this pass: #442 (resume teardown race in a test), #443 (HQ state docs), #444 (panel honesty about tunnel edit/delete), #441 (hosted `v0.3.0` upgrade evidence), #446 (S5-AUD-01 header-name fix, `read_request` fuzz target and seed corpus). Closed as stale/superseded/conflicting: #171, #343, #396, #397, #398, #399, #401-#406, #417, #422. **Only #445 stays open** — the Fleet CA/Join/Pairing stack is deferred to the next minor because its acceptance gates are open and the two-host LAB is suspended. Release `v0.3.1` was cut from reviewed `main` as a signed `v0.3.1-rc.2` and then Stable on the same commit (`3b8bcde`), with eight signed assets each; its evidence is hosted CI only, and the S5 waiver is recorded as residual risk in [RELEASE.md](RELEASE.md). The next mission after this release is the 0.4.0 queue in §9.5 (adaptive policy pack measurement first), with the Fleet stack and the two-host LAB gates after owner re-authorization.

**2026-11-26 post-release closure pass.** The `v0.3.1` GitHub release page now states its evidence level and known issues itself (hosted CI only; S5 OPEN; `ready` Unknown; Preview surfaces; #445 absent; S10; paused security review; one-hour soak cap). The published bytes were verified outside CI: `sha256sum -c SHA256SUMS` returned `OK` for all six listed artifacts and `cosign verify-blob` with the `release.yml@refs/tags/v0.3.1` identity returned `Verified OK` (exit 0). The twenty open issues were audited and split into non-LAB-gated delivery work and genuinely LAB-gated acceptance work (details in [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md)). PR #445 was merged up to `main` (`d0519be`) with the panel bundle rebuilt from source; its remaining acceptance gates are the two-host ones. **Mission after this pass:** re-open a LAB window to run the two-host acceptance for `v0.3.1` and Fleet Join, and obtain the independent S5 sign-off; until then the 0.4.0 policy-pack work proceeds on code and CI evidence only, with no claimed performance numbers.

The full verification block — merged PRs, the six S5 regression tests with their local results, the flaky `resume_tunnel` teardown test and its branch fix, the open-PR triage with the local Fleet-stack integration result, and the LAB limits — is maintained in one place only: [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md).

## 9. Replan after Stable 0.2.0 (2026-11-16)

This section is the current working order. It replaces the §5 MVP queue, which is complete except where noted. Nothing below is a claim of delivered capability; each item names the evidence it needs. Fixed constraints in §6 are unchanged.

### 9.1 Where the product stands

- **Released:** Stable `v0.2.0` (tag `76c12cb`, 2026-11-15). LAB evidence on the identical rc.12 binary: upgrade soak (1 of 272 probes lost, accepted as a journal-proven external path outage), S3 reboot plus real-browser pass 19/19, reload plus Integrity on protocol v2. Local browser only, not LAB: pairing 11/11, PKI 15/15, BAFT guided 12/12. Pre-Stable audit: no Critical/High/Medium.
- **Merged after the tag, in no release yet:** PR #255 (panel polish), PR #257 (README one-line pinned signed install), PR #258 (HTTPS panel for a domain or IP, `--domain` / `--ip`, menu after install). The `v0.2.0` bundle does not contain `panel-https.sh`, and real Let's Encrypt issuance is untested.
- **MVP queue leftovers:** B3 is closed as Unknown by decision (ADR-0030, no `CAP_NET_ADMIN`). B5 (history/alerts) and B6 (automated accessibility audit) are implemented with CI evidence only. The physical phone check is not a blocker.
- **Known items from the release notes:** no LAB browser run of pairing/PKI/BAFT on the new dashboard; open Low/Medium panel items in [PANEL_REDESIGN_PARITY.md](PANEL_REDESIGN_PARITY.md); wrong HTTP reason phrase on some agent error statuses; in-flight requests fail during a short path outage (detection about 20 s, PR #251); Integrity stays Unknown on protocol v1, which is still the `setup init` default; abroad-host drift (S10).

### 9.2 Release train

| Release | Theme | Exit gate |
|---|---|---|
| **0.2.1** | Ship what is merged, close the release-note known items, make the install path trustworthy | Signed rc, N-1 to N upgrade on LAB with a soak of at most 60 minutes, then Stable |
| **0.3.0** | Operability: what an operator with more than one tunnel and no shell habit needs | Each item has its own acceptance; LAB where a host is involved |
| **0.4.0** | Adaptive Traffic Policy Pack, phases 0 to 2 (measurement and engine-agnostic policy interface) | Measured against Baseline and Balanced; simulation labelled apart from LAB |
| **Later** | UDP/datagram carrier, Fleet/control plane, external security review before any v1.0 claim | Own ADR, audit and LAB each |

### 9.3 0.2.1 queue (in this order)

| ID | Work | Why | Effort | Evidence needed |
|---|---|---|---|---|
| R1 | Correct HTTP reason phrases on agent error statuses | Release-note item 6 | S | Unit test over every status the agent emits |
| R2 | Release bundle completeness: `panel-https.sh` is in the package list and the release smoke test fails without it (already on `main` through PR #258) | The `v0.2.0` bundle predates it | done on `main` | Confirmed when the 0.2.1 rc bundle is built and verified |
| R3 | HTTPS install acceptance. Today CI (`test-panel-https.sh`) proves nginx config, own-CA issuance, auth pass-through, idempotence and rollback with `certbot` shimmed. Add an unshimmed ACME run against a local ACME test server (Pebble) in CI | Real evidence for the ACME flow without an owner domain; a public Let's Encrypt run still needs an owner domain and stays labelled untested until then | M | CI; later one real-host run |
| R4 | LAB browser pass of pairing, PKI enrollment and BAFT guided setup on the new dashboard | Release-note item 3 | M | REAL-LAB run, one LAB user at a time |
| R5 | Panel Low/Medium review items (hardcoded capability pills, `\|\| 0` KPI defaults, colour-only legend, small chart text, drawer/RTL, toast live region, language switch rebuild) | Release-note item 4; several touch the no-fake-data rule | M | panel-ui job, axe audit |
| R6 | `v0.2.1-rc.1`, then 0.2.0 to 0.2.1 upgrade and rollback on LAB with soak, then Stable | S2 on every release | S | Release run, LAB run, checksums |

R1, R3 and R5 need no LAB and can run in parallel. R4 and R6 share the LAB and run one after the other.

### 9.4 0.3.0 candidates (ordered by operator value)

| ID | Work | Notes |
|---|---|---|
| O1 | Protocol v2 as the default for new pairs, with an explicit, visible downgrade to v1 for an old peer | Makes "Test tunnel" work out of the box. Needs an ADR-0028 amendment: no silent downgrade stays binding |
| O2 | More than one tunnel per host in the agent and panel (Tier L3) | Largest functional gap for real use; needs an ADR for instance ownership and the helper scope |
| O3 | Update check and guided update from the panel: shows the newer signed release, verifies checksum and signature, then runs the existing guarded update with automatic rollback | Reuses ADR-0025; no auto-update without the operator's confirmation |
| O4 | Certificate and CRL expiry alerts plus LAB evidence for B5 history/alerts | B5 has CI evidence only |
| O5 | Secret-free support bundle download from the panel | Shortens every troubleshooting round; must pass a no-key-material test |
| O6 | Reverse out of Preview: integrity probe for Reverse (ADR-0021) and an acceptance run | Reverse stays Preview until that run passes |
| O7 | Hardening carried from Phase 6: S4 fuzz depth for Apply body, bundle import and evidence readers; S7 IPv6 evidence; S9 fresh-VM acceptance; S10 drift attribution | S9 and S10 need the owner's hosts |

### 9.5 0.4.0 and later

- **Adaptive Traffic Policy Pack** (epic [#226](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/226), [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md), Proposed, nothing implemented): TP-0.1 (#227) research first, then measurement, then the engine-agnostic policy interface (#245) before any profile. Userspace only. No latency claim about the internet path.
- **UDP/datagram carrier** ([#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244)): separate transport feature with its own ADR, security audit and LAB. It is a prerequisite only for Gaming/Trading benefit claims; Balanced, Streaming and Bulk proceed on TCP/Yamux.
- **Fleet / control plane** (draft [PR #171](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/171), after 0.4.0; its ADR number collides with ADR-0029 on `main` and must be renumbered if revived) stays Draft and untouched. Fresh design (Proposed, F0): [ADR-0045](ADR-0045-fleet-control-plane.md).
- **External security review** was paused by the owner on 2026-11-11 and re-authorized on 2026-11-24 (S5 active, whole-`main` scope); it is required before any v1.0 claim.
- **Research (0.3.x/0.4.0): tcp-tls sustained transfers on the Iran path.** LAB evidence, 2026-11-16 16:00-17:50Z, Iran LAB host only, runs 43897433228, 43910396414 and forensics 43934069639: 2 MB probes over tcp-tls repeatedly stopped after about 0.5-1 MB. The abroad node logged "Connection reset by peer" while the Iran node timed out, with 0 restarts, flat fd/RSS and automatic recovery. The same binary passed 2 MB probes for one hour at 14:00-15:00Z. Not yet known: whether a middlebox causes it, whether it depends on time of day, flow length or TLS fingerprint, and whether other carriers behave differently. First step: measurement only (repeatable probe at several times of day plus a non-tunnel TLS bulk control). No censorship-resistance or workaround claim until it is measured. Related: Policy Pack TP-0.1 (#227).

### 9.6 Housekeeping

Draft PRs #197 (B4, superseded by merged #201), #173 (B1 docs, superseded) and #169 (HQ command-center foundation, removed from the project by the owner's decision on 2026-11-16) are closed; their branches are kept until a merged-PR mapping is verified. #171 (Central Fleet ADR) stays Draft and is scheduled after 0.4.0. [PROJECT_BOARD.md](PROJECT_BOARD.md) is refreshed; the body of HQ Issue #2 still describes rc.7 and the log is append-only, so the correction goes in as a new entry.

### 9.7 Status addendum (2026-11-17)

Evidence levels: CI ≠ VM ≠ LAB ≠ Production. Nothing below is production evidence. Main at the time of writing: `36107fb`.

**Releases**

| Release | State | Evidence |
|---|---|---|
| `v0.2.0` | Stable, released 2026-11-15 | [STABLE_0.2.0_EVIDENCE.md](STABLE_0.2.0_EVIDENCE.md) |
| `v0.2.1` | Stable, released 2026-11-16 | [STABLE_0.2.1_EVIDENCE.md](STABLE_0.2.1_EVIDENCE.md) |
| `v0.2.2` | **Not tagged.** Signed `v0.2.2-rc.3` on `5dd0890` passed the LAB checks (one soak accepted under the outage rule). The Stable tag push needs the owner's own approval and an admin machine; the owner has no access to that machine for now. | [STABLE_0.2.2_EVIDENCE.md](STABLE_0.2.2_EVIDENCE.md) |
| `v0.3.0` | **Not accepted.** Signed `v0.3.0-rc.1` on `69f43ed`: Reverse Preview, domain installs and the 0.3.0 operator features pass in LAB; the upgrade soak does not pass because of the aged-flow stall below. | [STABLE_0.3.0_EVIDENCE.md](STABLE_0.3.0_EVIDENCE.md) |

The README one-line install still pins `v0.2.1`. It moves to the next Stable tag only after that tag exists.

**Blocking finding: aged-flow throttling on the Iran path (research, cause not established).** Since about 16:00Z on 2026-11-16, large transfers over a long-lived Direct session (Iran dials abroad) are cut at about 0.6 to 1 MB with yamux timeouts on both nodes, on 0.2.2 and 0.3.0-rc.1 alike; Reverse is clean on the same path. LAB run [44056840174](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44056840174) (no BAFT-TUNNEL in the path, plain TLS between the two hosts, 2 MiB echo after the connection has aged) completed and, per the stall thread's report, shows a fresh connection finishing in under a second while connections aged 60 s or more need about 80 to 100 s for the same 2 MiB, with no transfer error. That points at a flow-age-dependent limit in the network path rather than at the tunnel engine, but the cause is not established: raw TCP versus TLS on aged flows is the open diagnostic (PR #313). No workaround, circumvention or censorship-resistance claim is made. Stable 0.3.0 waits for this to be understood or fixed.

**0.3.0 operator items (all merged):** O1 protocol v2 default (#268), O2 host tunnel list (#279, ADR-0036), O3 offline update notice (#275, ADR-0035), O4 certificate expiry warning (#270), O5 support bundle (#272), O6 Reverse Preview with two-node integrity probe (#282, #283, ADR-0037), O7 fuzz target for state files (#281). Panel display of the new items and the host list with two real instances are not LAB-tested.

**After 0.3.0 (state on main at 2026-11-18; Stable v0.3.0 is released; evidence levels never mix: CI is not VM, VM is not LAB, LAB is not production)**

| Area | State on main | Evidence | What is missing |
|---|---|---|---|
| Rakhsh integration | Independent rewrite (ADR-0038, ADR-0043); health feed #334, rotation/link-switch counts #335, backup peers in setup #336 | CI | No code copied; Rakhsh stays a separate AGPL project |
| Multi-link failover | `backup_remotes` in the dial path (#296, ADR-0039); link switches logged (#370) | LAB PASS, one run: 44298974821 (primary relay stopped, backup link carried the data) | Failback is not LAB-tested; one run only |
| Transport health | Model (#293), publishing (#306), kernel TCP_INFO feed (#334, ADR-0041) | LAB PASS, one run: 44304026997 (real RTT both nodes, healthy under load, unavailable after carrier cut) | Quality fields other than RTT stay Unknown without samples; single path |
| Stream resume | Protocol v3 wired into serve/forward loops (#365, ADR-0040), tcp-tls Direct only, default-off Preview | LAB PASS, one run: 44283746412 (16 MiB, relay cut twice, payload hash equal) | Stays default-off Preview; Reverse has no resume |
| Session rotation | Opt-in `session_rotate_secs` (#315, ADR-0049) | LAB PASS: 44121043530 (Noise-TCP Direct) and rotation probe harness #333 | Off by default; Iran path throttles flows older than about 15-20 s |
| Automatic pairing | One pairing code, no manual cert moving (#338, ADR-0050), Reverse and Iran/abroad questions (#348, #353), helper fixes #356-#360 | LAB PASS, one run: 44224936582 (Direct and Reverse, Reverse stays Preview) | Fleet and SSH enrollment are separate and still Proposed |
| v0.3.0 upgrade | Stable v0.3.0 released | LAB PASS, one run: 44271619864 (upgrade LAB) | Production evidence not claimed |
| Fleet and add server | Pure node status report (#347), hub listener (#381), member reporter (#382), `--fleet-config` wiring (#383, default off), two-host LAB harness (#385); ADR-0045 accepted with conditions; ADR-0051, ADR-0052, ADR-0053 all Proposed; server health card on the Servers page (#372, consent default off), probe_api and Globalping probe (#371, #374) | Report transport: LAB PASS, one run: 44338843969 (hub abroad, member Iran, mutual TLS; throwaway openssl PKI mirroring the fleet_ca profile, not fleet_ca-minted). Rest: CI | Enrollment (F-7, #384) is not wired; SSH enrollment and remote update stay disabled until the independent reviews |
| Panel | React panel default at `/`, classic at `/classic` (ADR-0054, [#350](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/350), merged `124c08c`); no fake data | CI | Physical phone check is not a blocker; classic removal is a later decision |
| Doctor | Offline read-only config and host checks (#298, ADR-0042) | CI; LAB PASS, one run: 44351670022 (`v0.3.1-rc.1` prerelease) | Broader LAB use not recorded |
| Policy Pack (0.4.0) | Interface behind `policy-pack` (#305, ADR-0044), status contract proposed (ADR-0047), simulated baseline (#310) | CI, simulated | Relay does not call the policy yet |
| Multi-link failback | `link_policy` supports it; `LinkSet` keeps it off | none | Backlog by coordinator decision 2026-11-18: failback closes a healthy session, low value against risk; no LAB planned |
| UDP carrier | Design only (ADR-0046, Proposed) | none | No carrier code |
| Competitive benchmark | Advisor assessment (#302); E2 harness (PR #308); E3 LAB harness (#332) | CI | E3 result (run 44124112450) |

**Documentation:** this addendum and the 0.3.0 evidence draft bring the roadmap to 2026-11-17; HQ Issue #2 gets a matching append-only entry. Firewall status stays Unknown (ADR-0030); no component takes `CAP_NET_ADMIN`.


## 9.8 Local Agent panel React modernization (2026-11-18)

**Decision accepted:** [ADR-0054 — React architecture for the local Agent panel](ADR-0054-react-local-agent-panel.md). The owner approved React + TypeScript, with Vite as the initial build-tool choice, for the existing local Agent panel. This supersedes ADR-0032's vanilla-only frontend technology constraint for this panel only; the Rust core, Agent API trust boundary, static deployment model and all non-conflicting guardrails remain binding.

**Scope:** the nine existing local routes only — Overview, Tunnels, Servers, Monitoring, Setup, Diagnostics, Activity, Agent and Settings. Central Fleet/control-plane work remains separate and is not authorized by this decision.

**Delivery order**
1. Produce three independent React visual prototypes using free/local tooling and permissively licensed components. Show all nine routes and compare the variants side by side. No paid AI generation, subscription or credit purchase is required or authorized.
2. Reuse the already-downloaded Datta Able Free React template as a visual reference; evaluate other free components only after license, maintenance, accessibility, RTL and dependency review.
3. Wait for the owner's selection before implementing a visual direction in the product.
4. Migrate the selected design route-by-route, retaining the existing authenticated Agent API and keeping the vanilla panel as fallback until parity is accepted.
5. Integrate a reproducible static build into the Rust Agent package; no Node.js runtime on target hosts.
6. Close functional/API, auth, fa/en RTL/LTR, responsive, accessibility, CSP, dependency/license, clean-install, upgrade/rollback and uninstall gates before removing the legacy panel.

**Historical snapshot (2026-11-21; superseded by §9.10):** the React panel was default at `/` and the classic panel remained at `/classic` (#350 merged). The earlier three-variant prototype PR [#345](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/345) was closed unmerged. The owner selected a custom charcoal enterprise-monitoring direction for **Overview only**; this does not select the remaining eight routes or waive their design/acceptance gates. At that time, implementation was in draft [PR #410](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/410), branch `ui/enterprise-monitoring-dashboard-20261120`, head `a55e8a21112f73fe46724176c2ffcaefde0deb16`, based on main `7d7d40f66fb7f8f4a9e384c0bf70c4d4278a252f`. Local TypeScript/Vite build, `check:dist`, formatting and diff checks passed. Desktop and mobile visual QA verified Persian RTL and a 390×844 CSS-pixel viewport without horizontal overflow. The preview used a local Agent API (`0.8.0-preview`) and a temporary dummy session token; the Geo endpoint explicitly labels its output as a visual preview fixture, not live Agent telemetry. This is not production or two-host LAB evidence. The four required exact-head GitHub workflows (`phase1-rust`, `phase0`, Same-SHA package/signature verification and CodSpeed) passed. No fabricated inventory/metrics are shipped; session/stream counters are cumulative, peer link details and firewall visibility remain Unknown under ADR-0030. PR #410 was Draft at that snapshot; no release or deployment was claimed.
**Documentation:** this table brings the roadmap to 2026-11-21. Firewall status stays Unknown (ADR-0030); no component takes `CAP_NET_ADMIN`; Reverse stays Preview.

## 9.9 Documentation addendum (2026-11-18)

- **Prerelease `v0.3.1-rc.1`** exists as a tag (`4c2a5bc`). It is a prerelease, not Stable. Upgrade and pairing LAB results for this tag are pending; no claim is made until they are recorded.
- **Doctor:** LAB PASS, one run: [44351670022](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44351670022).
- **Fleet:** the two-host report transport is LAB PASS on one run ([44338843969](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44338843969)). That run used throwaway openssl certificates, not `fleet_ca`-minted ones.
- **Enrollment and Add server:** enrollment ([#384](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/384)) and the operator API ([#388](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/388)) are on `main`. The Add-server UI ([#393](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/393)) has its confirm/sign path untested. The root signer helper is in draft [#394](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/394) and is not on `main`. Evidence for these is CI only.
- **Panel:** the React panel is the default at `/`; the classic panel stays at `/classic` for one release ([ADR-0054](ADR-0054-react-local-agent-panel.md)).
- SSH enrollment and remote update remain disabled. Reverse stays Preview. Firewall status stays Unknown (ADR-0030).

## 9.10 Panel integration and merge status (2026-11-22)

**Main:** `4db89f216254d17caff90e047b7e719b2d491377` — merge commit for [PR #423](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/423), merged 2026-11-22. The candidate head was `bbf0e771887bbb3bcbb3bf1610b767bc3242bb3b`; its first-parent integration commit includes the enterprise dashboard work from PR #410.

**PR #410:** [merged](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/410), not Draft/open. GitHub reports head `555c30367585090f7bd4adf76a9c22166e1862c7`, base `59a2802bc80b8177d92289d20f3dbe584e826a52`, and merge commit `bbf0e771887bbb3bcbb3bf1610b767bc3242bb3b` (the candidate commit subsequently merged by PR #423). The 2026-11-21 status paragraph in §9.8 is retained as a historical snapshot only; its PR state and old head/base description are superseded by this addendum.

**PR #423 validation:** all four required exact-head workflows passed on `bbf0e771887bbb3bcbb3bf1610b767bc3242bb3b` (`phase0`, `phase1-rust`, Same-SHA package/signature verification and CodSpeed). Local browser QA covered desktop, 390px mobile layout, Persian RTL, English LTR and font delivery. These checks are CI and local preview evidence only: the animation/fixture preview is not live Agent telemetry, and no two-host LAB, production deployment or release is claimed. The maintainer self-audit was a COMMENT, not independent third-party approval.

The enterprise dashboard is integrated into the default panel. The classic panel remains at `/classic`; remaining route parity and acceptance gates are not waived. Firewall visibility remains Unknown under ADR-0030; no component uses `CAP_NET_ADMIN`; Reverse remains Preview.
