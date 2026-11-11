# BAFT-TUNNEL — Product Roadmap and Verified Delivery Queue

**Status snapshot:** 2026-11-11 (U7 accepted; U8 operator-doc reconciliation in progress)
**Source baseline:** `main` at `a58dd88078a7db3426e105122acf811da5f83974` (PR #163 docs/status merge). U7 was intentionally pinned to product parent SHA `1219f547e6d9c2fe9605385f37a7d5dd8dab2995` (PR #162 host-side checksum fix). Trusted [U7 run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) completed successfully: release-tag verification, signed RC.7 asset verification and real two-host acceptance all passed.
**Gate status:** P0-1..P0-5 remain closed. D2 panel-managed `tcp-tls` CSR/certificate lifecycle is implemented and merged in [PR #150](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/150) (merge `e5a7451f259be7df14187a58db5fe1c29c51a81b`); exact-head CI/two-host evidence is recorded in that PR, and a trusted repeat on the immutable RC.7 candidate passed in [LAB run 42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181). U1 CRL renewal and real two-host push are proven by [LAB run 42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521) on ancestor code SHA `8be74376176442106669f4a81293f0865629ecaf`: CRL number `0x1002→0x1003`, 30-day validity, both nodes report `crl_state=ok`, 2 MiB tunnel recovery, short/replayed CRLs refused. The repeat D2 LAB reported `host_drift=none`; the earlier unrelated nftables delta was not reproduced, but its source remains unattributed (S10).
**Release:** signed prerelease `v0.1.0-rc.7` is published; [release](https://github.com/zarkmakerburg/BAFT-TUNNEL/releases/tag/v0.1.0-rc.7). `rc.6` is historical and must not be used because its Cosign identity was invalid under the current tag policy. No stable release or production deploy.
**Product status:** Active development / pre-release; not production-ready. `ready` remains Unknown unless independently verified under the readiness contract. Main is protected; all code changes continue through PRs and exact-SHA CI.

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
| 1 — Secure runtime | Rust TCP runtime, identity, authorization, PKI, installer, systemd, failure and adversarial tests | **U7 signed-RC.7 integrated two-host MVP acceptance passed** ([run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428)); bounded Direct scope only | U8 operator docs must match the accepted path; keep explicit scope limits |
| 2 — Engine framework | StreamEngine, engine selection, Noise direct, BAFT process adapter and pinned Store | **Implemented in code; evidence gates differ by engine** | Noise stays Experimental; BAFT Store path got its own real-host proof before Wizard exposure (exposed as Preview, Direct only in PR #133) |
| 3 — CLI and local Agent | Setup plan/init/join, local API, desired-state validation, preflight, Apply/Verify/Receipt | **Local stages implemented** | Service lifecycle, authenticated peer/session and data-path verification accepted |
| 4 — Local panel | Overview, Servers, Tunnels, Wizard, Agent, Activity, Settings; fa/en and responsive behavior | **Installed lifecycle, diagnostics and operational workflow passed the trusted two-host U7 repeat; remaining product-quality gaps are B1–B6** | UI status and actions match a fully verified backend; no placeholder inventory/metrics |
| 5 — Additional transports | WebSocket/TLS, uTLS sidecar, QUIC/UDP, Relay and auto-rotation | **Future work, not MVP** | Each candidate has a real use case, ADR, security/licence review and acceptance matrix |
| 6 — Hardening and stable | Bounded reliability (maximum 60-minute soak), backup/restore, update lifecycle, fuzz depth, independent security review (P0+P1 surface assessed 2026-11-09; full pre-stable review paused by owner on 2026-11-11) and signed stable release | **Open; U7 included one 3600-second Direct soak; no multi-hour soak is planned** | All release gates closed and required independent review explicitly re-authorized/completed |

### Engine capability matrix

| Engine | Direct | Reverse | Product exposure |
|---|---|---|---|
| `tcp-tls` | Supported; real two-host integrity/recovery evidence exists; two-host Direct ops acceptance passed ([LAB run 42635190485](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42635190485), main `0b299d0`) | Real data path; dedicated LAB pass (run 42506805986) plus R3 breadth LAB pass (run 42525790470, P0-4) | Direct supported; Reverse **Preview** |
| `noise-tcp` | Experimental; Direct two-host evidence exists | Unproven; earlier label-only result is not valid data-path evidence | Direct Experimental; Reverse unavailable |
| `baft` | Separate-process adapter and launcher; Store→Launcher path, store rollback and signed release asset passed real-host LAB (P0-3) | BAFT's own behavior is separate and must not inherit Rust Reverse claims | Independent acceptance passed 2026-11-09 ([LAB run 42552860541](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42552860541), main `3a95c7f`): may be shown as **Preview, Direct only** (see [BAFT_ENGINE_ACCEPTANCE.md](BAFT_ENGINE_ACCEPTANCE.md)). Listed that way in `/v1/engines` and the Wizard since PR #133 (`c7dbd15`); setup cannot create its configuration yet (plan B4) |
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
- ~~Audit actor/target completeness~~ **Done** (audit v2, PR #109). Structured Preflight/Diagnostics evidence codes + params and fa/en localization are implemented in draft PR #164; exact-head CI and U8 acceptance remain pending (B1).
- Since U2 (PR #137, LAB 42692143748) the agent is a systemd service under its own user and setup sets owners itself. U1 CRL renewal/push is accepted (LAB 42656507521); U6 signed packaging and quick installer are complete in prerelease `v0.1.0-rc.7`.
- No customer/subscription/reseller/billing system in core.
- No production release. Independent security assessment of the P0+P1 surface done 2026-11-09 (`docs/SECURITY_ASSESSMENT.md`): 3 Medium and 3 Low fixed (#112, #114, main `f3c8b54`), 4 Low/Info open or accepted; no Critical/High found. This is not the pre-stable review of the full product.

### Panel implementation truth

The current MVP panel source is plain static HTML/CSS/JavaScript in `panel/`, served by the agent with no framework/build chain. Some high-level architecture/README text still describes React/TypeScript/Vite; reconcile those descriptions with the shipped source rather than assuming a React migration has happened. Do not add a frontend migration to the P0 queue.

## 5. Prioritized delivery queue

### Current queue: usable MVP first (verified refresh 2026-11-11, `main` `a58dd88`)

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
| now | U8 | Final operator quickstarts en/fa and troubleshooting follow the exact accepted U7 path; eliminate contradictory legacy operator instructions | [PR #165](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/165); exact-head CI/review pending | — |
| after U8 | B1–B6 | B1 structured localized evidence is implemented in draft PR #164; exact-head CI rerun after one test assertion correction. Then B2 integrity probe, B3 read-only firewall visibility, B4 BAFT configuration, B5 history/alerts, B6 accessibility | as listed | — |
| Phase 6 | S1–S10 | Stable hardening; no soak longer than 1 hour is planned. Keep external nftables attribution as a separate open environmental item (S10) | ≤60 min where soak is needed | owner review currently paused |

Not before the MVP queue is done: P1-1 enrollment and P2 Fleet/Jobs (Tier L in the plan).

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
5. Measured throughput/byte counters and history with units, interval, provenance and freshness. — **Done** (PR #109, panel PR #120); history beyond one hour is B5.
6. Machine-readable Preflight/Diagnostics evidence codes + params and localized explanations. — _Implemented in draft PR #164; CI and U8 acceptance pending_: B1.
7. Read-only firewall visibility. — _Not started_: B3.
8. Safe “Add from Link” import with versioned schema, checksum, strict validation and preview. — _Not started_; delivered as the panel bundle import in U5b.

### P2 — multi-node operations

1. Fleet inventory, Node groups, version and health freshness.
2. Remote orchestration via scoped Jobs, idempotency, receipts and rollback.
3. Update/rollback and backup/restore; require restore acceptance, not just backup success. — **Done locally** (ADR-0025; restore accepted on LAB in runs 42635190485 and 42619848756, same version); cross-version update and remote Jobs open.
4. Monitoring, alerts and operational history.
5. Third-party integration API for external customer/subscription panels if the owner prioritizes it.

### P3 — new protocol/transport options

WebSocket/TLS, uTLS sidecar, QUIC/UDP, Relay, multi-engine failover and automatic rotation are gated research. Do not begin them while P0 remains open unless there is an explicit owner priority change and a written acceptance plan. Do not claim traffic invisibility or censorship resistance.

## 6. Fixed constraints

1. Rust remains the core language; Go does not enter the core.
2. BAFT stays a separate pinned process with its own lifecycle and licence/attribution obligations.
3. Hashem/BCC/BAFT lessons are patterns, not source-code transfer.
4. TCP-TLS Reverse remains Preview; Noise Reverse remains unavailable.
5. BAFT acceptance gates passed on 2026-11-09 for Preview/Direct only ([BAFT_ENGINE_ACCEPTANCE.md](BAFT_ENGINE_ACCEPTANCE.md)); PR #133 (`c7dbd15`) lists it as Preview, Direct only in `/v1/engines` and the Wizard, but setup still refuses to create its configuration (plan Q4, B4). It is never Stable or Reverse without further evidence.
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


## 2026-11-11 U7 acceptance addendum

Trusted U7 [run 43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428) completed all three jobs successfully on product parent SHA `1219f547e6d9c2fe9605385f37a7d5dd8dab2995`: release-tag ancestry checks, signed RC.7/RC.5 asset verification, and the real two-host U7 acceptance. The final marker was `PHASE1_TWO_HOST_E2E=PASS` and `OPS_RESULT` recorded `integrity_64k_2m=1`, `cross_version_update=1`, `backup_restore=1`, `bad_update_rolled_back=1`, `revoked_refused=1`, `uninstalled=1`, `soak_s=3600`, `rounds=264`, `failed_probes=0`, `restarts=0->0/0->0`, and `baft_fw_rules=0`. Diagnostics integrity and receipt ready remained Unknown by design. The run did not exercise CRL push (`crl_push=0`); U1 remains separately evidenced by run 42656507521. The abroad host showed six lines of unrelated address-set drift; its source is unknown and remains S10. No firewall neutrality, Reverse, Noise, BAFT, Stable or production claim is made.
