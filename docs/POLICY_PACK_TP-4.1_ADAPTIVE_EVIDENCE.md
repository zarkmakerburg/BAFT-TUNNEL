# TP-4.1 adaptive controller: simulated evidence record

**Status: harness evidence only (`simulated`), one host, loopback. TP-4.1 stays open on its
two-host LAB criterion. The loop measured here made no change, and since 2026-11-27 the live 1 Hz
driver itself is covered end to end (section 5) — with scripted sample values, because this harness
cannot produce a live congestion estimate.**
Item: TP-4.1 (#238). Design: [ADR-0056](ADR-0056-adaptive-optimization-controller.md) decisions 4
(inputs), 6-8 (hysteresis, steps, fail-safe) and 12 (this benchmark). Row table:
[ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decision 15. Status contract:
[ADR-0047](ADR-0047-policy-status-contract.md).

This record exists to keep the claim honest. It reports what the wired loop did on this box, which
is **nothing**: every sample's queueing-delay estimate was `0`, no change was accepted, and the
adaptive arm is level with the static row it starts from inside each run's spread. Section 3
explains why the estimate stayed at zero — that explanation is the useful result of this slice and
the concrete input for TP-4.2.

## 1. What was run

- Harness: `src/policy_baseline.rs`, test `policy_baseline::tests::adaptive_controller_experiments`
  (`cargo test --release --features policy-pack -- adaptive_controller_experiments --ignored
  --nocapture`), `POLICY_ADAPTIVE_SECONDS=8`, `POLICY_ADAPTIVE_LONG_SECONDS=180`.
- Harness revision: `4a20d1df` (the commit that adds the wiring and this arm).
- Review follow-up: the commit after it re-caps a write's offer to the live quantum, makes a
  re-offer replace rather than add its queued span, and feeds a no-delivery tick as a missed
  sample instead of a zero. None of those changes any number below: the run delivered bytes in
  every sampled direction and made no change, so only the no-traffic path behaves differently.
- Arms, both built from the shipped table by test-only constructors, no hand-copied numbers:
  - `sched-w262144-q16384-cap65536` — the static `balanced/1` row (control).
  - `adaptive-w262144-from-balanced/1` — the same row plus the ADR-0056 loop, which may only move
    down the ladder (`gaming/1`, `trading/1`) and must restore configured `balanced/1`.
- Path: the product's `yamux_config` and the shipped `relay_with_policy` on both ends in one
  process, joined by a userspace rate-limiting proxy over loopback TCP. The adaptive loop runs on
  the client session's two arbiters. No TLS/Noise handshake, no NIC, no second host.
- Workload: three bulk flows, each with its own echo stream, ping interval 20 ms, shaped at
  250 000 / 500 000 / 1 000 000 B/s shared by the three flows on the one connection, two repeats per arm and rate, plus one 180 s run at
  250 000 B/s so a down-step could reach the 30 s dwell and the 60 s verification window.
- Environment: four-core Linux `4.19.0-gvisor` sandbox (`x86_64`), one process, no other load,
  2026-11-26. One invocation, 300.95 s, run `tp-4.1-local-2026-11-26`.

## 2. What the loop saw and did

| Rate (B/s, shared connection) | Arm | flow p50, median (range) ms | flow p95, median ms | CPU, median µs | sender handoff, median MB | split |
|---|---|---|---|---|---|---|
| 250 000 | static `balanced/1` | 2 829.6 (2 534.1–2 990.9) | 3 269.4 | 480 000 | 3.52 | 0.948 |
| 250 000 | adaptive | 2 686.2 (2 210.0–3 046.5) | 3 236.6 | 405 000 | 3.59 | 0.896 |
| 500 000 | static `balanced/1` | 1 655.8 (1 649.4–1 665.9) | 1 673.1 | 520 000 | 5.16 | 0.931 |
| 500 000 | adaptive | 1 648.8 (1 642.3–1 650.8) | 1 676.2 | 530 000 | 5.16 | 0.927 |
| 1 000 000 | static `balanced/1` | 863.9 (862.4–865.6) | 907.6 | 810 000 | 8.37 | 0.933 |
| 1 000 000 | adaptive | 865.1 (864.1–867.5) | 905.6 | 800 000 | 8.44 | 0.933 |

- **Changes: 0 in all 13 runs.** No `policy adaptive:` line was printed, no row moved, and the
  change/gap lists are empty everywhere.
- **Samples: 14–17 per 8 s run and 266 in the 180 s run**, i.e. the 1 Hz driver ran as designed;
  `estimate_p50`, `estimate_p95` and `estimate_max` were `0 ms` in every run and
  `estimate_over_50ms` was `0`, so the three-sample persistence above the 50 ms step-down threshold
  was never reached.
- **Cost/benefit: level.** Each pair sits inside the other arm's spread; the long run (p50 3 232 ms,
  p95 3 310 ms, 45.07 MB handed off, 6.44 s CPU) is the static row's own latency under this
  workload. No benefit is claimed and none was observed; the constant cost is one 1 Hz timer and
  two counter reads per second when the opt-in is on.

## 3. Why the estimate stayed at zero (the slice's real finding)

The ADR-0056 decision-4 sample is the arbiter's `queued_bytes` divided by the interval's granted
rate. In this scheduler the queue is a rendezvous, not a buffer: `FlowHandle::acquire` sends a
request, the arbiter task calls `serve()` at the top of its loop and grants it immediately, and only
then blocks on `recv()`. The request is therefore queued for microseconds, and a 1 Hz sample
almost always sees `total_queued_bytes() == 0`. The bytes that actually wait — the ones already
granted whose `write_all` is blocked by the peer's Yamux window — are not in the queue and are not
counted by this signal, so a run whose ping p50 is 2.8 s reports a 0 ms queueing estimate.

This is not a wiring defect: the counters are read from the live arbiter, the driver runs at the
ADR's cadence, and the decision core is exercised by its unit tests (ladder, deadband, dwell, rate
limit, verification/rollback, cooldown, fail-safe, stale reset, log line). It is a property of the
input the ADR fixed, and it is recorded rather than papered over. Consequence: on this harness the
loop cannot be observed changing rows at all. The wiring's live path is covered by
`src/policy_relay.rs`'s tests: an accepted change reaches exactly one direction's arbiter and its
read chunk, the opt-in gates the controller, and — since 2026-11-27 — the real 1 Hz task applies a
change and a fail-safe restore end to end (section 5).

## 4. What is not measured

- The **two-host LAB window**, suspended by the owner's decision: carrier RTT, path change,
  impairment, both engines' real tuples and the loop's behaviour on a real path.
- Number and spacing of changes over a 10-minute run, time-to-restore after the load stops, and
  every rollback/suspension path **on live traffic** — none occurred (0 changes), so these remain
  test-covered only. They are TP-4.2's matrix, not this record's.
- Delivered goodput (the harness counts sender handoffs), RSS, the 60-minute profile run.
- `violations` was a constant `0` when this record was written. **Updated 2026-11-27:** the arbiter
  now counts a request above the configured row's cap (`ArbiterStats::guard_violations`) and the
  controller's sample carries that counter instead of a literal zero. Evidence labels: the counter
  and its propagation are **tested** (`only_an_over_cap_request_counts_as_a_guard_violation`,
  `the_live_sample_carries_the_arbiters_guard_counter`, and
  `a_live_arbiter_guard_violation_restores_the_row_without_forced_samples`, which raises the
  signal through the live arbiter and reads it back with the session's own tick); the 1 Hz driver
  inside a running product is **tested** end to end with scripted sample values (section 5); and no
  live congestion sample exists, so decision 4's queue-delay signal stays **unverified** here. `overflow_refusals` is live
  as before.

## 5. The live driver, tested end to end (2026-11-27)

The gap this section closes was stated plainly in section 3: the wiring was covered by tests that
call the session's tick directly, so the task that actually ticks once a second was read from code
and never observed. `PolicySession::new(config, profile, true)` spawns that task;
`the_live_1hz_driver_applies_a_change_and_restores_the_configured_row_on_guard` now lets it run for
real and asserts, from outside:

| Behaviour | Observation |
|---|---|
| An accepted decision reaches `Command::Configure` | Three consecutive live ticks above the 50 ms threshold move one rung (`balanced/1` → `gaming/1`); the read bound becomes 4 KiB **and** the running arbiter's next grant drops from 16,384 to 4,096 bytes — the `Command::Configure` the driver sent, not just the controller's state. |
| The failure path restores the last-good row | A guard violation on a later live tick restores `balanced/1` in the controller, the read bound and the arbiter's grant, with `cause=guard_violation` in the transcript. |
| The loop is inert without the opt-in | `the_live_driver_does_not_run_without_the_opt_in` builds the same session with `policy.adaptive` off: no controller, no task, the configured row unchanged, and the scripted samples never consumed after 1.5 s. |

The sample **values** are scripted (a test-only queue in `PolicySession`), because this harness
cannot produce a live estimate at all — that is section 3's finding. What the test therefore proves
is the driver's cadence, the decision, the apply and the fail-safe on the product's own relay path;
what it does not prove is that real traffic produces the signal, which stays decision 4's open
question and the LAB window's to settle. Three reverse mutations were run and each made the test
fail before being restored: removing the `arbiter.configure` call (grant stayed 16,384 where 4,096
was asserted), making `fail_safe` suspend without restoring (no second change within the 15 s
bound), and spawning the driver regardless of the opt-in (the inert test's script was consumed).

No profile becomes `measured` through this record, ADR-0048's default-off decision and the
`simulated` label of every other record are untouched, and the `policy.adaptive` opt-in stays off
by default because there is no reproducible benefit to turn it on.
