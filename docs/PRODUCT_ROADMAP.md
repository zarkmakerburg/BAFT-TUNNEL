# BAFT-TUNNEL — Product Roadmap (phases, MVP, engines)

Thin Persian pointer (no status of its own): [PROJECT_BOARD.md](PROJECT_BOARD.md).

Reconciled 2026-11-08 against main `82e8768`. This file is the single source of truth for phases, engines and next steps; evidence (run IDs, SHAs) lives in `PHASE1_IMPLEMENTATION_STATUS.md`; `PROJECT_BOARD.md` is the Persian task view and must follow this file. Status: **not production-ready**. Pre-release `v0.1.0-phase1-pre` is published and cosign-signed.

## Inputs behind this revision
- Research (2026-11-07): no tunnel protocol is undetectable; Reality and WireGuard are blocked in Iran. Strategy is **several swappable engines plus fast rotation**, not one "invisible" protocol.
- Core stays Rust (ADR-0003). Camouflage engines that need uTLS/Reality-style fingerprints may run as an out-of-process Go sidecar behind the same engine interface (ADR-0018, proposed).
- Owner decisions: BAFT and C-BAFT code may be imported after a health check and licence/secret review (never secrets). The BAFT main engine is an engine option. Wizard: engine, then direct/reverse, then one port field; everything else automatic. TCP/TLS and Noise+TCP are in scope; UDP/QUIC planned.
- Owner authorized merge/tag/release and standing isolated LAB tests (no secrets printed, cleaned up).

## MVP definition (v0.2.0)
A non-expert operator can, on two Ubuntu 22.04 hosts, install a signed release, run a guided setup (engine, direct/reverse, one port), get a working mutually authenticated tunnel, see health, and switch engine in under a minute. Concretely:
1. Phase 1 gates closed (below).
2. Engine interface with ≥2 working engines: TCP+TLS1.3 mTLS (exists) and Noise+TCP (promoted from prototype).
3. `baft-tunnel setup` guided CLI producing validated config and PKI material.
4. Local web panel (read-only status plus the wizard) on the mockup design, Persian RTL and English.
5. Signed release, upgrade and rollback tested.

Not in MVP: central multi-user control plane, UDP/QUIC, camouflage sidecar.

## Phases
| Phase | Goal | Exit |
|---|---|---|
| 0 Foundation | Done | merged |
| 1 Core runtime (close) | Real-host soak/impairment, CRL distribution, CA/pin rollover, adversarial coverage, acceptance | all gates PASS with SHA |
| 2 Engine framework | Engine trait and registry, config `engine:` field, BAFT main engine and Noise+TCP, rotation policy, API/Agent contracts | two engines pass LAB E2E; ADR-0018 accepted |
| 3 Agent and local API | Node agent, Axum API, signed desired state, audit, `setup` CLI | MVP items 3, 5 |
| 4 Web panel | React/TS/Vite panel, wizard, dashboard, diagnostics, RTL | MVP item 4; v0.2.0 |
| 5 Camouflage and transports | Go uTLS sidecar engine (ADR), WebSocket/TLS front, UDP/QUIC, auto-rotation | engine matrix tested under impairment |
| 6 Hardening and stable | Fuzz in CI, long soak, upgrade/restore tests, docs, signed v1.0 | all release gates |

Engine rollout order: (1) TCP+TLS mTLS, (2) Noise+TCP, (3) BAFT main engine (after health check), (4) WebSocket/TLS front, (5) uTLS sidecar, (6) QUIC/UDP. Rotation: operator-defined ordered engine list per tunnel; automatic switch on health failure.

## Actual state at 2026-11-08 (evidence, not intent)
Evidence kinds are kept apart: **code** (in main), **CI** (hosted runners, Ubuntu 24.04 and 22.04/systemd 249), **LAB** (two real hosts, isolated, cleaned up).

| Area | Code | CI | LAB | Notes |
|---|---|---|---|---|
| Rust TCP runtime, mTLS 1.3, CRL, pins | yes | yes | yes | Direct and Reverse |
| Offline PKI, installer, systemd, `LoadCredential=` on 22.04 | yes | yes | yes | P1-000 |
| Soak and impairment (TLS, Direct) | yes | n/a | PASS 30 min with app-level impairment; PASS 3600 s; PASS 2% real TCP loss | longer than 1 h and Reverse soak not run |
| CRL distribution, CA/pin rollover | yes (`push-crl.sh`, bundle CRLs, two pins) | mock tests | rollover sequence PASS | `push-crl.sh` not yet run on real hosts |
| Adversarial handshake tests | yes | yes | n/a | P1-003 |
| Fuzz targets | **no** | no | n/a | P1-005 open; no `fuzz/` directory |
| Engine trait, `engine:` config | yes (ADR-0018) | yes | covered by TLS runs | no fallbacks or supervisor |
| Agent, local API, `setup` CLI, enrollment | **no** | no | no | Phase 3 not started |
| Web panel | mockup only (draft PR #17, fake data) | no | no | no backend |
| Signed release, upgrade, rollback | release pipeline and `rollback.sh` yes | partial | no | no automated upgrade/rollback test |
| Independent security review | waived by owner, not performed | | | |

## Engine matrix
| Engine | Direct: code / CI / LAB | Reverse: code / CI / LAB | Under loss or soak | MVP role | Wizard |
|---|---|---|---|---|---|
| `tcp-tls` (TLS 1.3 mTLS) | yes / yes / PASS | yes / yes / PASS | Direct only: 2% loss PASS, 3600 s PASS | required | shown |
| `noise-tcp` | yes / yes / PASS (enrolled key relays, unenrolled refused) | yes / unit only / PASS (run 42457633244, enrolled relays, unenrolled refused) | not run | required (second engine) | shown as experimental |
| `baft` (imported main engine) | no | no | no | later (v0.3): health check P2-002 first | hidden |
| `ws-tls` | no | no | no | later | hidden |
| `utls-sidecar` | no | no | no | later (ADR needed) | hidden |
| `quic` / UDP | no | no | no | later | hidden |
| Fallback list and auto-rotation supervisor (ADR-0018 item 2) | no | no | no | later (v0.3); MVP switches engine manually | hidden |

No engine is claimed undetectable.

## Dependency-ordered path to MVP (v0.2.0)
1. **M0 Noise Reverse LAB: DONE 2026-11-08** (run 42457633244 on main). MVP now has two engines with Direct and Reverse LAB evidence.
2. **P3-001 `setup` CLI** (config, key material, pairing). Everything user-facing (API, wizard) reuses its code, so it comes first. This is the top blocker; see [PLAN-P3-001-setup-cli.md](PLAN-P3-001-setup-cli.md).
3. **P3-002 Agent and local API** (status, validated desired state, audit). Depends on the config model from P3-001.
4. **P4-001 Panel connected to the API** (read-only status plus wizard). Depends on P3-002; reuses mockup PR #17.
5. **R-001 Upgrade and rollback test, signed v0.2.0.** Pipeline and `rollback.sh` exist; the missing part is an automated test and a documented procedure. Can run in parallel with 3 and 4.
6. **P1-005 Fuzz targets with CI smoke run.** Independent; run in parallel.
7. **A-001 Integrated LAB acceptance:** clean Ubuntu 22.04 hosts, signed release installed, tunnel set up only through the guided path, probe passes, engine switch measured.

Not in MVP: central control plane, UDP/QUIC, camouflage sidecar, `baft` engine, fallback supervisor, multi-hour soak (P6).

## IN PROGRESS
- **P3-001** `setup` CLI: plan written; S1 (`setup plan`) done (PR #42); S2 (`noise-tcp` init/join) done (PR #43); S3 (`tcp-tls` from `baft-pki.sh` output) done (PR #44); S4 (LAB) PASS, run 42461145830 on `54bebff`: noise-tcp install 38 s, engine switch to tcp-tls 42 s, 3 commands per host (bundle copy and checksum comparison manual). P3-001 closed.
- **P3-002** Agent and local API: plan in [PLAN-P3-002-agent-api.md](PLAN-P3-002-agent-api.md); A1 (read-only status API) done (PR #47); A2 (validated desired state + audit) done (PR #48); A3 (LAB) PASS, run 42466445718 on `d05b727`. P3-002 closed; next P4-001 panel on the API.

## TO DO (priority order)
1. **P3-001** slices S1 to S4 (plan file).
2. **P3-002**, **P4-001**, **R-001**, **P1-005** as in the path above.
3. **A-001** integrated LAB acceptance, then tag v0.2.0.
4. After MVP: P2-002 BAFT health check, fallback supervisor, ws-tls, sidecar, QUIC, P6 long soak, independent security review before any v1.0.

## DONE
Phase 0 (PR #1); Phase 1 runtime, PKI, installer, systemd; P1-000; P1-001 (soak 3600 s, 2% real loss, impairment); P1-002 (ADR-0017: dual pins, CA bundle with per-CA CRLs, freshness report, push helper, rollover LAB PASS); P1-003; P1-004 closed with documented exceptions (2026-11-08); review findings 1,2,3,4,7; PR #3; release pipeline and pre-release; P2-001 (ADR-0018); P2-003 (engine trait); P2-004 (`noise-tcp` behind the trait, Direct and Reverse LAB PASS); M0.

## Documented exceptions carried forward
Soak beyond 1 h; `push-crl.sh` on real hosts; Noise under packet loss and soak; independent security review (waived, not performed); fuzzing (P1-005).

## Acceptance gates
| Gate | Status |
|---|---|
| Phase 1 core (runtime, mTLS, PKI, installer, 22.04 path, E2E, soak, loss, rollover) | PASS with documented exceptions |
| Fuzz targets in CI | OPEN (P1-005) |
| Second engine with equal evidence | PASS (M0, Direct and Reverse) |
| Guided setup, API, panel | NOT STARTED (P3-001 next) |
| Upgrade and rollback test | OPEN (R-001) |
| MVP v0.2.0 | NOT DONE |
| Production release | NOT DONE |

## Fixed rules
1. Never import secrets. BAFT/C-BAFT code import is allowed after review (owner decision 2026-11-07, overrides the former "no BAFT code" rule).
2. Ubuntu 22.04 / systemd 249 is the default baseline; no OS/systemd upgrades.
3. No completion claims without SHA plus CI or LAB evidence; simulation, hosted CI and real-host LAB are different evidence.
4. LAB changes isolated and cleaned up; never print secrets or host addresses in public docs.
5. One micro-task at a time; touch only files named by the task.
