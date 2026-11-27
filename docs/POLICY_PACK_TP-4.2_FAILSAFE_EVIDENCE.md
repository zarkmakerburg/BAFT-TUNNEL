# TP-4.2 rollback and fail-safe matrix: forced evidence record

**Status: the matrix is delivered through the wired path, with a forced (`simulated`) timeline; TP-4.2
stays open on its two-host LAB criterion.** Item: TP-4.2 (#239). Design:
[ADR-0056](ADR-0056-adaptive-optimization-controller.md) decisions 7 (verified, reversible steps),
8 (fail-safe), 9 (one log line per change) and 12 (benchmark plan, amendment of 2026-11-27 moving
change spacing, time-to-restore and the rollback/suspension paths here). Rows:
[ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decision 15.

This record covers the part of #239 that a single host can settle: every rollback and fail-safe
branch, forced through the session the product actually runs — controller, `Command::Configure`,
arbiter, read chunk — instead of through the controller's return value alone. It also states what a
forced sequence cannot settle and leaves that to the suspended LAB window.

## 1. What was run

- Matrix harness: ordinary tests in `src/policy_relay.rs` (they run in every `policy-pack` test
  invocation, no `--ignored`). `PolicySession::without_driver` builds the real session — both
  arbiters, the real controller, the real apply path — and each test forces one tick per simulated
  second through `PolicySession::tick_with`, the same decide-and-apply half the 1 Hz driver calls.
  There is no clock, sleep, socket or proxy in the matrix; the counters are the only input.
- Tests, one per matrix row: `a_failed_verification_rolls_the_live_arbiter_and_read_bound_back`,
  `a_guard_violation_restores_the_live_row_and_suspends_the_loop`,
  `an_overflow_restores_the_live_row_and_suspends_the_loop`,
  `a_failed_row_stays_cooled_down_across_a_suspend_and_resume_cycle`,
  `a_stale_gap_resets_the_streak_and_restores_the_live_row`,
  `forced_fail_safe_samples_change_nothing_without_the_opt_in`,
  `the_forced_matrix_timeline_is_reproducible`,
  `forced_condition_matrix_covers_clean_congested_lossy_jittery_and_path_change`,
  `forced_flapping_run_is_bounded_and_settles`.
- Regression arm: the TP-4.1 loopback benchmark re-run on this change
  (`POLICY_ADAPTIVE_SECONDS=6 POLICY_ADAPTIVE_LONG_SECONDS=20 cargo test --release --features
  policy-pack --bin baft-tunnel adaptive_controller_experiments -- --ignored --nocapture`), to show
  that adding the test seam left the live sampling path unchanged. Every line is `simulated`.
- Environment: four-core Linux `4.19.0-gvisor` sandbox (`x86_64`), one process, no other load,
  2026-11-27, run `tp-4.2-local-2026-11-27`.

## 2. The matrix (forced)

Each row reads "forced sequence → asserted result". Latencies are the forced sample values, seconds
are forced tick positions; nothing here is a network observation.

| # | ADR-0056 | Forced sequence | Asserted result |
|---|---|---|---|
| 1 | d7 revert on failed verification | `balanced/1`, 100 ms at 1–3 s, then a 5% improvement at 63 s | one step to `gaming/1` at 3 s; `no_improvement` revert to `balanced/1` at the 60 s window, arbiter grant 4 KiB → 16 KiB and read chunk 4 KiB → 16 KiB |
| 2 | d8 guard fail-safe | `gaming/1` at 3 s, `violations` +1 at 4 s | `guard_violation` restore to `balanced/1` on that tick; 299 further congested samples to 304 s change nothing; first voluntary step after the 300 s suspension at 306 s |
| 3 | d8 overflow fail-safe | same with `overflow_refusals` +1 | `overflow` restore on that tick, same suspension and read-bound restoration |
| 4 | d7+d8 cooldown survives suspension | `bulk/1` → `streaming/1` at 3 s, fails verification at 63 s, violation at 64 s | `streaming/1` cooled to 663 s; no change at any sample 65–662 s, including the first 300 s of resumed sampling after the suspension ends at 364 s; retried at exactly 663 s with `queue_delay_high` |
| 5 | d8 stale gap | `streaming/1` → `balanced/1` at 3 s; one missed tick at 6 s; three misses at 9–11 s | the gap drops the streak (4 congested samples across it, still one change); the third consecutive miss restores `streaming/1` with `stale_signals`, grant and chunk back to 32 KiB |
| 6 | d9/d10 inert without opt-in | 5 samples of 400 ms with 9 overflow refusals and 9 violations, `policy.adaptive` off | no change, no estimate, chunk and arbiter grant stay `balanced/1`'s 16 KiB |
| 7 | d7+d12 timeline | `bulk/1`, 100 ms at 1–3 s, 60 ms at 63 s, calm at 64–66 s | step at 3 000 ms; the 40% improvement keeps it; return to `bulk/1` at 66 000 ms; change gap 63 000 ms |
| 8 | #239 conditions | clean (60 s of 5 ms); congested; lossy (one 80 ms burst every 3 s for 120 s); jittery (45/55 ms alternating 120 s); path change (permanent 100 → 15 ms) | clean, lossy and jittery: 0 changes; congested: one step; path change: the step is kept and the row returns to `balanced/1` at 65 s |
| 9 | #239 flapping, long run | 4 s congested / 8 s calm, 600 s (the plan's ceiling) | exactly two changes (step at 3 s, `no_improvement` revert at 63 s), each one rung, never above configured; nothing after 63 s because the failed row's 600 s cooldown outlasts the run, so the loop settles instead of oscillating |

The unit tests in `src/policy_adaptive.rs` remain the coverage for the pure decision rules
(deadband, dwell, rate limit, ladder, log line). The rows above are the same rules exercised through
the wiring, which is what TP-4.1 could not observe: a change reaching the arbiter and the read
bound, and a restore landing on the configured row.

## 3. What is forced and what is measured

Every millisecond in §2 is a scripted tick. What the matrix proves is the wiring: each decision is
applied to the direction that caused it, through `Command::Configure`, with the read chunk moved
with it, and every fail-safe restores the configured row rather than the previously verified one.
What it does not prove is that such sequences arise on a real path: a forced sample is not evidence
that a congested, lossy or jittery carrier produces it at 1 Hz, because the live signal is the
decision-4 estimate whose zero-reading is TP-4.1's recorded finding.

§4 is measured on this box, and it says only that the added test seam left the live path unchanged.

## 4. Regression arm (measured, `simulated`)

Same arms and scenario as TP-4.1's record (`adaptive_control`, three flows, ping interval 20 ms),
shorter runs: 6 s per arm and rate, two repeats, plus one 20 s run at 250 000 B/s. `flow p50` and
`flow p95` are medians over the three flows, then over the two repeats.

| Rate (B/s) | Arm | flow p50, median (range) ms | flow p95, median ms | samples | changes |
|---|---|---|---|---|---|
| 250 000 | static `balanced/1` | 2 838.7 (2 735.8–2 941.6) | 3 220.8 | — | — |
| 250 000 | adaptive | 2 747.0 (2 663.2–2 830.7) | 3 202.3 | 12 | 0 |
| 500 000 | static `balanced/1` | 1 527.6 (1 417.2–1 638.0) | 1 650.1 | — | — |
| 500 000 | adaptive | 1 634.8 (1 632.8–1 636.9) | 1 647.2 | 14 | 0 |
| 1 000 000 | static `balanced/1` | 854.8 (853.1–856.4) | 865.4 | — | — |
| 1 000 000 | adaptive | 854.4 (852.8–856.0) | 866.2 | 12 | 0 |

- **Changes: 0 in all seven arms**, and every sample's estimate was `0 ms`
  (`estimate_p50`/`p95`/`max` = 0, `estimate_over_50ms` = 0), the 20 s run included (35 samples).
  This is the TP-4.1 finding reproduced, not a new one: the arbiter queue is a rendezvous, so the
  1 Hz sample reads zero.
- **Latency is level**, inside each pair's spread; the two arms differ by no more than the run-to-run
  spread TP-4.1 already reported. No benefit is claimed, and the arms are shorter than TP-4.1's, so
  these numbers are not directly comparable to that record.

## 5. What is not measured

- **The two-host LAB window** (suspended by the owner's decision): whether decision 4's queueing
  sample is usable on a real path at all — the thing that decides whether any row can move in
  production — plus real loss, jitter, path change, carrier kill, oscillation on live traffic,
  time-to-restore after a real load stops, and change spacing on a live signal.
- **Live `violations`:** the scheduler path instantiates no `Guarded<TrafficPolicy>`, so the guard
  sample is a constant `0` and row 2 is forced only, never observed.
- **#239's "mark adaptive `Unknown`/`Off`":** there is no status surface to mark. ADR-0056
  decision 11 reserves the adaptive view for [ADR-0047](ADR-0047-policy-status-contract.md), which
  is still Proposed. The requirement is recorded as blocked on ADR-0047 and is not invented here;
  it is not one of TP-4.2's remaining criteria.

No profile becomes `measured` through this record, ADR-0048's default-off decision and the
`simulated` label of every other record are untouched, and `policy.adaptive` stays off by default.

## 6. Status

The matrix, #239's multi-condition, flapping and ten-minute tests, and the forced timeline are
shipped behind the existing gates. TP-4.2 stays **open** with exactly one remaining criterion: the
two-host LAB window, like TP-3.2–TP-3.5 and TP-4.1. #239 stays open.
