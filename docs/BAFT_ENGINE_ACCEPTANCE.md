# BAFT engine — independent acceptance for Wizard exposure

Status: criteria defined; verdict section is filled from LAB evidence (see "Verdict").
Scope: the pinned BAFT main engine (`id: baft`, Go, `h2` mTLS carrier only) run through the Engine Store and Launcher. No new engine or transport. No stable tag, release or deploy.
Rule: BAFT stays hidden from `/v1/engines`, Wizard and preflight until every **Required** criterion below is met on an exact SHA.

## Criteria

| # | Criterion | Required | Evidence source |
|---|-----------|----------|-----------------|
| A1 | Release artifact verified: `SHA256SUMS` cosign signature verifies only for `release.yml@refs/tags/vX.Y.Z[-...]` (SA-03); identities from `refs/heads/main`, feature branches, and foreign repositories are **rejected**; engine digest equals `release/engine-manifest.json` pin | yes | LAB workflow step `COSIGN_IDENTITY_NEGATIVES=PASS` |
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

## Verdict (2026-11-09)

Evidence: [LAB run 42552860541](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42552860541), main `3a95c7f1e0be880dcce1e8ca731bc2302bd94500`, engine from signed prerelease `v0.1.0-rc.4` (digest `e4f04837…3647` = manifest pin), two real LAB hosts (Ubuntu 22.04 / systemd 249), result `PHASE1_TWO_HOST_E2E=PASS`. PR #116 CI (rust, ubuntu-2204-baseline, lab-config) green before merge.

| # | Result | Evidence |
|---|--------|----------|
| A1 | PASS | Signed-release step passed with the identity restriction and the new negative controls (foreign repo and feature-branch ref must not verify; the step exits non-zero otherwise) |
| A2, A3 | PASS (earlier, unchanged inputs) | `baft-engine-conformance` workflow; pin and manifest unchanged |
| A4 | PASS | `BAFT_SYSTEMD_RESULT hardened_direct_ok=1 mdwe=yes no_seccomp_kill=1 store_launch_ok=1` |
| A5 | PASS | `BAFT_RESULT direct_ok=1`, 64 KiB + 2 MiB integrity |
| A6 | PASS | `unauthorized_refused=1 wrong_pin_refused=1 revoked_refused=1 revoked_listener_refused=1 bad_hash_refused=1 store_pin_mismatch_refused=1` |
| A7 | PASS | `BAFT_STORE_ROLLBACK_RESULT … rollback_to_a=1 tampered_rollback_refused=1 a_carried_2mib_after_rollback=1` |
| A8 | PASS | listener restart recovery 34 s (plain) / 43 s (systemd); engine SIGKILL recovery 35 s |
| A9 | PASS (bounded) | 1200 s soak, 84 rounds, fd 18→18 on both sides, RSS 23.5→25.2 MB (dialer) and 23.6→29.0 MB (listener); 2% real TCP loss, 6 packets dropped, data still carried, nft table removed |
| A10 | PASS | stage cleanup; loss table removed; no stray processes reported |
| A11 | DOCUMENTED LIMIT | BAFT has no control API or counters; status must stay `unknown` |
| A12 | DOCUMENTED LIMIT | BAFT "reverse" in the LAB is a swapped dialer, not ADR-0021 Reverse; Direct only |

### What may be shown

- **BAFT may appear in the Wizard and `/v1/engines` as Preview, Direct only**, behind an explicit "Preview" label, using the signed-release Store → Launcher path only (pinned digest, no manual binary paths).
- It must show: traffic and end-to-end status are **unknown** for this engine; process crash resets active flows; Reverse unavailable; h2 mTLS carrier only.
- It must **not** be shown as Stable, as production-ready, or as supporting Reverse.

### What remains (not claimed)

1. ~~Wizard/capabilities exposure~~ Done: `/v1/engines` lists `baft` as `preview`, Direct only, and the Wizard shows it with a Preview label and the unknown-status limits. Wizard setup/preflight still blocks BAFT (the planner cannot create its configuration yet), so it is displayed but not configurable from the Wizard.
2. No multi-hour soak; the 20-minute soak and 2% loss on a short flow are bounded evidence (only 6 packets dropped).
3. No BAFT status/diagnostics integration (needs a probe-based design); no preflight check specific to the BAFT engine.
4. Listener RSS grew about 23% over the soak; watch it in a longer soak before Stable.
5. Hosted-runner and LAB evidence only; unrelated firewall drift on the abroad host was seen earlier and is not claimed absent.
