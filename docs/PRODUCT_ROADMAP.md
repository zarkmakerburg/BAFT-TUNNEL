# BAFT-TUNNEL — Product Roadmap (phases, MVP, engines)

Task-level board: [PROJECT_BOARD.md](PROJECT_BOARD.md).

Updated 2026-11-07. Source of truth for phases and tasks; evidence lives in `PHASE1_IMPLEMENTATION_STATUS.md`. Status: **not production-ready**. Pre-release `v0.1.0-phase1-pre` is published and cosign-signed.

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

## IN PROGRESS
- **P1-001** Soak and impairment on real LAB (optional soak/impairment merged in #15; run and record evidence).
- **P1-004** Phase 1 acceptance tracking.

## TO DO (priority order)
1. **P1-002** CRL distribution and CA/pin rollover. ADR-0017 accepted (A1+B1); implement.
2. **P1-005** Fuzz targets (frame, hello, config) and CI smoke run (review finding 8).
3. **P1-004** Acceptance closure with latest SHA; Issue #2 body update.
4. **P2-001** ADR-0018 engine framework (trait, registry, Go sidecar boundary, rotation) (accepted).
5. **P2-002** Import health-check of BAFT / C-BAFT engine code (licence, secrets, quality) — report only.
6. **P2-003** Engine trait refactor of existing TCP+TLS path, behaviour unchanged.
7. **P2-004** Promote Noise+TCP to an engine behind the trait.
8. **P2-005** Phase 2 API/Agent/desired-state contracts.
9. **P3-001** `setup` guided CLI. **P3-002** Agent and local API. **P4-001** Panel from mockup (PR #17).
10. **P5-xxx** Sidecar, WebSocket/TLS front, QUIC/UDP.
11. **P6-001** Fuzz/soak in CI, upgrade/restore tests, docs, v1.0.

## Review finding 8 (release, fuzz, soak planning)
Release pipeline done (`release.yml`, cosign keyless). Fuzz: P1-005. Soak: P1-001 now, repeated at P6.

## DONE
Phase 0 (PR #1); Phase 1 runtime, PKI, installer, systemd, LAB Direct/Reverse/revocation PASS; P1-000 (Ubuntu 22.04 `LoadCredential=`); P1-003 adversarial handshake tests; review findings 1,2,3,4,7 (PR #13, #14); PR #3 merged; release pipeline and pre-release v0.1.0-phase1-pre.

## Acceptance gates
| Gate | Status |
|---|---|
| Rust TCP runtime, mTLS, PKI, installer | PASS |
| Ubuntu 22.04 credential path (hosted CI + LAB) | PASS |
| Two-host E2E Direct/Reverse/revocation | PASS |
| Soak and impairment on real hosts | OPEN (P1-001) |
| Remote CRL distribution, CA/pin rollover | OPEN (P1-002, ADR-0017) |
| Fuzz targets in CI | OPEN (P1-005) |
| Phase 1 acceptance | OPEN |
| Production release | NOT DONE |

## Fixed rules
1. Never import secrets. BAFT/C-BAFT code import is allowed after review (owner decision 2026-11-07, overrides the former "no BAFT code" rule).
2. Ubuntu 22.04 / systemd 249 is the default baseline; no OS/systemd upgrades.
3. No completion claims without SHA plus CI or LAB evidence; simulation, hosted CI and real-host LAB are different evidence.
4. LAB changes isolated and cleaned up; never print secrets or host addresses in public docs.
5. One micro-task at a time; touch only files named by the task.
