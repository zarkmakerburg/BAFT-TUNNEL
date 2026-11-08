# BAFT-TUNNEL — Product Roadmap and Verified Delivery Queue

**Status snapshot:** 2026-11-08  
**Source baseline:** `main` at `05a2b8078cc7afae751520209dba7fd474914bfb` (PR #86).  
**Open parallel work:** [PR #88 — Apply start stage through a root path-unit control helper](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/88), head `969154c0b02dee5656ba91cf410a8be3da006061`; not merged and real-systemd LAB not run.  
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
| 6 — Hardening and stable | Longer reliability, backup/restore, update lifecycle, fuzz depth, independent security review and signed stable release | **Open** | All release gates closed and independent review completed |

### Engine capability matrix

| Engine | Direct | Reverse | Product exposure |
|---|---|---|---|
| `tcp-tls` | Supported; real two-host integrity/recovery evidence exists | Real data path; one dedicated LAB pass on run 42506805986 | Direct supported; Reverse **Preview** |
| `noise-tcp` | Experimental; Direct two-host evidence exists | Unproven; earlier label-only result is not valid data-path evidence | Direct Experimental; Reverse unavailable |
| `baft` | Separate-process adapter and launcher code exist; previous BAFT process LAB pass | BAFT's own behavior is separate and must not inherit Rust Reverse claims | Hidden from `/v1/engines` and Wizard until Store/release acceptance |
| `ws-tls`, `utls-sidecar`, `quic` / UDP | Not implemented/advertised | Not implemented/advertised | Hidden / future |

**Evidence caveats**
- TCP-TLS Reverse Preview: [LAB run 42506805986](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42506805986), commit `2d8cd5e`. This is a bounded two-host pass, not soak, packet-loss or production evidence.
- BAFT engine process under hardened systemd 249: [LAB run 42504156305](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42504156305), commit `2d8cd5e`. This predates PR #85's Store→Launcher change and **does not verify the new Store execution path**.
- Direct real 2% TCP-loss, 3600-second soak, CA/pin rollover and earlier setup/agent/panel runs are in [PHASE1_IMPLEMENTATION_STATUS.md](PHASE1_IMPLEMENTATION_STATUS.md). Their exact historical SHAs and scope remain part of the evidence; do not silently relabel them as new tests on current HEAD.
- PR #86 added local Apply init/join and panel receipt flow; its PR description reports local tests/browser QA and a temporary live-agent receipt, but no real-host LAB for that Apply flow.
- PR #88 is open; its description reports fake-controller tests and staged installer checks, while explicitly stating no real-systemd host test was run.

## 4. Current implementation baseline (main at 05a2b807)

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
- PR #88 Apply service-start helper is not merged and has no real-systemd LAB evidence.
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
| P0-1 | PR #88 systemd control helper | Security review; real Ubuntu 22.04/systemd 249 LAB for start/stop, scope restriction, invalid/unregistered instance refusal, failure rollback, idempotency/stale requests, no unrelated state change and cleanup |
| P0-2 | Apply lifecycle and readiness truth | Keep init/join receipt semantics; distinguish service active, listener accepting, authenticated peer, authorized session and bidirectional data integrity; unknown stays Unknown |
| P0-3 | BAFT Engine Store → Launcher | Real-host store install/verify/start/restart-recovery/wrong-pin refusal/rollback test using pinned release manifest; record whether artifact is signed release or local build |
| P0-4 | TCP-TLS Reverse R3 | Agent restart, repeat/reconnect, stale-session replacement, port conflict, longer soak, packet loss, failure interruption and cleanup; Reverse remains Preview until accepted |
| P0-5 | Integrated signed-artifact MVP LAB | Clean two-host installation using signed release, supported setup path, accepted lifecycle, authenticated tunnel, 64 KiB + 2 MiB integrity, negative identity/pin/revocation cases, recovery, rollback/uninstall and cleanup |

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
