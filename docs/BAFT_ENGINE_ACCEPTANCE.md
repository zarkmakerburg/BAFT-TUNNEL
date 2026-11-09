# BAFT engine — independent acceptance for Wizard exposure

Status: criteria defined; verdict section is filled from LAB evidence (see "Verdict").
Scope: the pinned BAFT main engine (`id: baft`, Go, `h2` mTLS carrier only) run through the Engine Store and Launcher. No new engine or transport. No stable tag, release or deploy.
Rule: BAFT stays hidden from `/v1/engines`, Wizard and preflight until every **Required** criterion below is met on an exact SHA.

## Criteria

| # | Criterion | Required | Evidence source |
|---|-----------|----------|-----------------|
| A1 | Release artifact verified: `SHA256SUMS` cosign signature verifies for `release.yml@refs/heads/main` or `@refs/tags/v*` (SA-03 identity restriction); a foreign-repository identity and a feature-branch ref are **rejected**; engine digest equals `release/engine-manifest.json` pin | yes | LAB workflow step `COSIGN_IDENTITY_NEGATIVES=PASS` |
| A2 | Engine build is reproducible and licence package present (AGPL-3.0 source link, third-party licences) | yes | `baft-engine-conformance` job `BAFT_REPRODUCIBLE=PASS` |
| A3 | Config rendered by Rust validates in the real BAFT binary (golden configs) | yes | `baft-engine-conformance` job `golden` |
| A4 | Install path: Store install, verify, hardened systemd 249 launch on Ubuntu 22.04 (real LAB hosts) | yes | `BAFT_SYSTEMD_RESULT`, `BAFT_STORE_ROLLBACK_RESULT` |
| A5 | Traffic: 64 KiB and 2 MiB integrity through the engine (Direct) | yes | `BAFT_RESULT direct_ok=1` |
| A6 | Refusals: wrong identity, wrong pin, revoked client, revoked listener, wrong binary hash, wrong store pin | yes | `BAFT_RESULT` fields, `store_pin_mismatch_refused=1` |
| A7 | Update/rollback: release B over A, pins follow `current`, rollback to A, tampered previous refused | yes | `BAFT_STORE_ROLLBACK_RESULT` |
| A8 | Recovery: listener restart, engine SIGKILL under systemd | yes | `restart_recovered=1`, `engine_kill_recovered=1` |
| A9 | Stability: soak with flat fd/RSS on the BAFT flow, and real TCP packet loss on the BAFT flow, with data still carried and nft table removed | yes | `BAFT_SOAK_RESULT`, `BAFT_LOSS_RESULT` |
| A10 | Cleanup: no stray processes, units, users or nft tables after the run | yes | stage cleanup output |
| A11 | Status honesty: BAFT has no control API and no traffic counters; the product must report engine health/traffic as **unknown** (never "healthy"/"authorized" without a probe) | yes (documented limit) | `docs/BAFT_ENGINE_INTEGRATION_DESIGN.md`, ADR-0019 clause 4 |
| A12 | Direction honesty: the BAFT "reverse" LAB case swaps which host dials; it is **not** the ADR-0021 Reverse (service node dials out). Wizard must offer BAFT as Direct only | yes | ADR-0021, this document |

## Known limits that apply regardless of verdict

- A BAFT process crash resets active flows (upstream has no process-restart resume); the dialer recovers with a fresh session via systemd restart.
- No per-tunnel traffic counters or end-to-end status from BAFT; tunnel status stays `unknown` for the BAFT engine.
- Key files must be 0600 for the engine user (BAFT rejects group-readable keys).
- Only the `h2` mTLS carrier is accepted. `ws` and `utls` carriers stay hidden.
- Hosted-runner and LAB evidence only; not production evidence. Unrelated firewall drift on the abroad LAB host was observed earlier and is not claimed absent.

## Verdict

_Filled after the exact-SHA LAB run (see HQ Issue #2 and the Roadmap)._
