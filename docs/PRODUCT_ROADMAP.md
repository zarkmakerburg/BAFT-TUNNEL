# BAFT-TUNNEL — Product Roadmap and Verified Delivery Queue

**Status snapshot:** 2026-11-09  
**Source baseline:** `main` at `92b0645f8104694bbe9d016e24f7ec0c8f43eeb0` (PR #106).  
**Gate status:** all five P0 gates (P0-1 to P0-5) are closed with exact-SHA run evidence (see section 5 and [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2)). PR #88 (systemd control helper, ADR-0023) is merged. Signed prerelease `v0.1.0-rc.4` is the clean-install candidate; `rc.3` is superseded (glibc 2.39, unusable on Ubuntu 22.04). No stable tag, release or deploy. P1/P2 are held until owner review.
**Repository governance:** `main` is protected (2026-11-09): PR required, 0 approvals, required checks `rust`, `ubuntu-2204-baseline`, `lab-config`, no force push or deletion, admin bypass allowed.
**Product status:** Active development / pre-release. **Not production-ready.** The only published release state recorded in the repository is pre-release `v0.1.0-phase1-pre`.

This file is the single source of truth for phases, engines and delivery order. Detailed cross-project extraction, verified code state and implementation instructions: [CROSS_PROJECT_CAPABILITY_ALIGNMENT.md](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md). Exact historical test evidence and run IDs: [PHASE1_IMPLEMENTATION_STATUS.md](PHASE1_IMPLEMENTATION_STATUS.md). Persian task pointer: [PROJECT_BOARD.md](PROJECT_BOARD.md). HQ decisions and append-only evidence log: [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).

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
| 1 — Secure runtime | Rust TCP runtime, identity, authorization, PKI, installer, systemd, failure and adversarial tests | **Core implemented; acceptance history recorded with explicit exceptions** | Re-run any required regression on exact release candidate; preserve open exceptions |
| 2 — Engine framework | StreamEngine, engine selection, Noise direct, BAFT process adapter and pinned Store | **Implemented in code; evidence gates differ by engine** | Noise stays Experimental; BAFT Store path gets its own real-host proof before Wizard exposure |
| 3 — CLI and local Agent | Setup plan/init/join, local API, desired-state validation, preflight, Apply/Verify/Receipt | **Local stages implemented** | Service lifecycle, authenticated peer/session and data-path verification accepted |
| 4 — Local panel | Overview, Servers, Tunnels, Wizard, Agent, Activity, Settings; fa/en and responsive behavior | **Core local panel implemented; operational gaps remain** | UI status and actions match a fully verified backend; no placeholder inventory/metrics |
| 5 — Additional transports | WebSocket/TLS, uTLS sidecar, QUIC/UDP, Relay and auto-rotation | **Future work, not MVP** | Each candidate has a real use case, ADR, security/licence review and acceptance matrix |
| 6 — Hardening and stable | Longer reliability, backup/restore, update lifecycle (ADR-0025: scripts and Ubuntu 22.04 rollback test implemented, evidence below), fuzz depth, independent security review and signed stable release | **Open** | All release gates closed and independent review completed |

### Engine capability matrix

| Engine | Direct | Reverse | Product exposure |
|---|---|---|---|
| `tcp-tls` | Supported; real two-host integrity/recovery evidence exists | Real data path; dedicated LAB pass (run 42506805986) plus R3 breadth LAB pass (run 42525790470, P0-4) | Direct supported; Reverse **Preview** |
| `noise-tcp` | Experimental; Direct two-host evidence exists | Unproven; earlier label-only result is not valid data-path evidence | Direct Experimental; Reverse unavailable |
| `baft` | Separate-process adapter and launcher; Store→Launcher path, store rollback and signed release asset passed real-host LAB (P0-3) | BAFT's own behavior is separate and must not inherit Rust Reverse claims | Hidden from `/v1/engines` and Wizard until Store/release acceptance |
| `ws-tls`, `utls-sidecar`, `quic` / UDP | Not implemented/advertised | Not implemented/advertised | Hidden / future |

**Evidence caveats**
- TCP-TLS Reverse Preview: [LAB run 42506805986](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42506805986), commit `2d8cd5e`. This is a bounded two-host pass, not soak, packet-loss or production evidence.
- TCP-TLS Reverse R3 (P0-4): [LAB run 42525790470](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42525790470), commit `5a72a25`: 5 service SIGKILL cycles (max recovery 3 s), public SIGKILL (3 s), 30 s blackhole (8 s), mid-transfer kill (2 s, user connection completed), 2% real packet loss, 1200 s soak with flat fd/RSS, busy port / wrong identity / wrong pin / revoked refused, no stray processes or nft rules. Reverse stays **Preview** until owner review; not production evidence.
- BAFT engine process under hardened systemd 249: [LAB run 42504156305](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42504156305), commit `2d8cd5e`. This predates PR #85's Store→Launcher change. The Store path was then proven separately (next bullet).
- Backup/Restore and Update/Rollback (ADR-0025, PR #111, main `8e4f797`): CI on PR head `6e029ee`, `ubuntu-2204-baseline` [run 42548499998](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42548499998) on real Ubuntu 22.04 / systemd 249: guarded update, crashing build rolled back automatically (old binary serving again, config/keys digest unchanged), tampered restore refused without stopping the service, destroyed config/PKI/key restored and service back, no private key material in logs or journal; `lab-config` run 42548500098 (staged tests). Hosted-runner evidence only; no two-host LAB run, no stable tag/release/deploy. Limits: health check proves the process stays up, not data flow; rollback restores binary and unit only.
- BAFT Engine Store → Launcher (P0-3): [run 42510577167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42510577167) on `2d2718b` (local-build engine; store launch, kill recovery, wrong pin refused) and [run 42522105698](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42522105698) on `06c273e` with the **signed `v0.1.0-rc.2` release asset** (cosign + SHA256SUMS verified, digest = `engine-manifest.json` pin `e4f04837…3647`): store install/verify, hardened systemd 249 launch, 2 MiB data, listener and engine-kill recovery, wrong pin refused, release B over A, pin follows `current`, rollback to A, tampered previous refused, A carries 2 MiB after rollback, cleanup OK. LAB gate only; BAFT stays hidden from `/v1/engines` and Wizard until a separate owner-approved exposure gate.
- Direct real 2% TCP-loss, 3600-second soak, CA/pin rollover and earlier setup/agent/panel runs are in [PHASE1_IMPLEMENTATION_STATUS.md](PHASE1_IMPLEMENTATION_STATUS.md). Their exact historical SHAs and scope remain part of the evidence; do not silently relabel them as new tests on current HEAD.
- PR #86 added local Apply init/join and panel receipt flow; its PR description reports local tests/browser QA and a temporary live-agent receipt, but no real-host LAB for that Apply flow.
- PR #88 is merged (2026-11-08). Its own description reported fake-controller tests only; the real-systemd 249 gate was closed afterwards (P0-1, run 42523536944, SHA 22e94f1).

## 4. Current implementation baseline (main at 92b0645)

### Implemented in current main
- Rust data plane with TLS 1.3 mutual authentication, peer identity validation, SHA-256 pinning, CRL-aware certificate checks, bounded Yamux streams and reconnect behavior.
- `tcp-tls` and experimental `noise-tcp` Direct engine paths behind the engine configuration.
- Actual `tcp-tls` Reverse mode, surfaced as Preview through capability contract, Preflight and Wizard. Noise Reverse remains blocked.
- BAFT main engine as a supervised external process with pinned binary configuration. PR #85 also wires the hash-checked Engine Store's `current` binary into the launcher.
- Local CLI and local authenticated Agent API.
- Preflight, setup planner, local desired-state validation and audit.
- Apply `init` / `join` with server-side checks, explicit confirmation, idempotency, file verification, safe rollback of only files created by the operation and secret-free receipt.
- Dependency-free static local panel served by the agent, with Persian RTL and English LTR, desktop/mobile navigation and honest empty/unknown/offline states.

### Not implemented or not yet accepted
- Apply `start` (PR #88 helper) is merged and LAB-proven on two hosts (P0-5), but it is not enabled at boot and Apply never claims `ready`; receipt `ready` stays Unknown by design.
- No end-to-end authenticated peer/session/data-path readiness result in the current status API; local listener acceptance is not tunnel health.
- No central online enrollment/control plane or enrolled multi-node inventory.
- The Servers page is not a Hashem-style Fleet; it has local/configured-peer information only.
- No live throughput counters exposed in the API; traffic KPI remains unavailable.
- No read-only firewall inspection and no firewall mutation.
- Audit actor/target completeness and machine-readable/localizable evidence codes remain gaps.
- No customer/subscription/reseller/billing system in core.
- No production release and no independent security review.

### Panel implementation truth

The current MVP panel source is plain static HTML/CSS/JavaScript in `panel/`, served by the agent with no framework/build chain. Some high-level architecture/README text still describes React/TypeScript/Vite; reconcile those descriptions with the shipped source rather than assuming a React migration has happened. Do not add a frontend migration to the P0 queue.

## 5. Prioritized delivery queue

### P0 — current critical path

| ID | Work | Required result / exit evidence |
|---|---|---|
| P0-1 | PR #88 systemd control helper | **Real-systemd 249 gate closed 2026-11-08** (hosted runner, run 42523536944, SHA 22e94f1; two-host LAB proof rides on P0-5). Original criteria: Security review; real Ubuntu 22.04/systemd 249 LAB for start/stop, scope restriction, invalid/unregistered instance refusal, failure rollback, idempotency/stale requests, no unrelated state change and cleanup |
| P0-2 | Apply lifecycle and readiness truth | **LAB gate closed 2026-11-09**: staged receipt with authorized-session indicator (run 42524638625, SHA 231ba9d; hosted systemd 249) and the two-host proof in P0-5 run 42538458094 (main 92c4e70, release rc.4): `apply_start_serve/forward=applied`, `receipt_session=observed`, 64 KiB + 2 MiB `integrity_64k_2m=1` measured by the LAB stage, `receipt_ready=unknown`. Apply itself never claims data integrity or `ready`; it stays Unknown by design and is measured by Verify/LAB. Original criteria: Keep init/join receipt semantics; distinguish service active, listener accepting, authenticated peer, authorized session and bidirectional data integrity; unknown stays Unknown |
| P0-3 | BAFT Engine Store → Launcher | **LAB gate closed 2026-11-08** (runs 42510577167, 42522105698; signed rc.2 asset). Exposure in Wizard is a separate gate |
| P0-4 | TCP-TLS Reverse R3 | **LAB gate closed 2026-11-08** (run 42525790470, SHA 5a72a25; Reverse stays Preview pending owner review). Original criteria: Agent restart, repeat/reconnect, stale-session replacement, port conflict, longer soak, packet loss, failure interruption and cleanup; Reverse remains Preview until accepted |
| P0-5 | Integrated signed-artifact MVP LAB | **LAB gate closed 2026-11-09** (run 42538458094, main `92c4e70`, signed prerelease v0.1.0-rc.4 `c1652caa…f00f`; init/join through the guided CLI, start through Apply; unrelated host firewall drift observed on abroad, no BAFT-related firewall change). Original criteria: Clean two-host installation using signed release, supported setup path, accepted lifecycle, authenticated tunnel, 64 KiB + 2 MiB integrity, negative identity/pin/revocation cases, recovery, rollback/uninstall and cleanup |

**Execution order:** P0-1 → P0-2 → P0-3 → P0-4 → P0-5, unless an independent task can be done without sharing LAB hosts or conflicting files. Run only one real-host LAB workflow at a time.

### P1 — product operations and diagnostics

1. Authenticated Node enrollment and small remote Node inventory, only after a reviewed control-plane trust-boundary ADR.
2. Real authenticated peer/session/data-path diagnostic endpoint; TCP-only reachability must remain a lower evidence tier.
3. End-to-end tunnel state and last-verified timestamp.
4. Complete audit actor, target and resource identity.
5. Measured throughput/byte counters and history with units, interval, provenance and freshness.
6. Machine-readable Preflight evidence codes and localized explanations.
7. Read-only firewall visibility.
8. Safe “Add from Link” import with versioned schema, checksum, strict validation and preview.

### P2 — multi-node operations

1. Fleet inventory, Node groups, version and health freshness.
2. Remote orchestration via scoped Jobs, idempotency, receipts and rollback.
3. Update/rollback and backup/restore; require restore acceptance, not just backup success.
4. Monitoring, alerts and operational history.
5. Third-party integration API for external customer/subscription panels if the owner prioritizes it.

### P3 — new protocol/transport options

WebSocket/TLS, uTLS sidecar, QUIC/UDP, Relay, multi-engine failover and automatic rotation are gated research. Do not begin them while P0 remains open unless there is an explicit owner priority change and a written acceptance plan. Do not claim traffic invisibility or censorship resistance.

## 6. Fixed constraints

1. Rust remains the core language; Go does not enter the core.
2. BAFT stays a separate pinned process with its own lifecycle and licence/attribution obligations.
3. Hashem/BCC/BAFT lessons are patterns, not source-code transfer.
4. TCP-TLS Reverse remains Preview; Noise Reverse remains unavailable.
5. BAFT remains hidden from capability discovery and Wizard until its independent acceptance gates pass.
6. No fake success, stale-green UI, unsupported action button or unverified engine option.
7. Apply/Job operations must be authorized, scoped, idempotent where possible, verifiable, receipted and recoverable.
8. No firewall/routes/sysctl/production-host changes by default.
9. Use Ubuntu 22.04 / systemd 249 as the baseline; no OS/systemd upgrades as a shortcut.
10. One LAB run at a time; cleanup is part of acceptance.
11. No production release until explicit release gates and required owner authorization are satisfied.

## 7. Release gate checklist

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
