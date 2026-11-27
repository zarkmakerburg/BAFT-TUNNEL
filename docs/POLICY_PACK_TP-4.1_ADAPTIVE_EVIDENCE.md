# TP-4.1 adaptive controller: simulated evidence record

**Status: delivered and merged 2026-11-27. The loop measured here made no change, and the live 1 Hz
driver itself is covered end to end (section 5); since the decision-4 amendment (section 6) the
sample carries granted-but-unwritten bytes, and the live driver was observed producing a non-zero
estimate (1,001 ms, `in_flight_bytes` 16,384) at the exact live state where `queued_bytes` reads 0 —
on this loopback harness, with a test-chosen drain rate. The decision-12 benchmark was then executed
with that signal (section 7): at 250 / 500 / 1000 KB/s the loop settles exactly one rung below the
configured row, p95 estimates are 187 / 93 / 53 ms, the estimate recovers in 0.6-1.6 s after the load
stops, and the 150 s run returns to the configured row 5.9 s after it — level RTT, 0.4-1.7% fewer
bulk bytes, **no benefit claimed**. TP-4.1 has no open repo-side work left.
What remains unverified is real-path behaviour, which needs the owner-suspended two-host LAB window
(a release-level gate shared with TP-3.2-TP-3.5); it is recorded as unverified, not claimed, and
this `simulated` record does not substitute for it.**
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

## 6. The in-flight sample at the relay boundary (2026-11-27, ADR-0056 decision 4 amended)

Section 3's finding was that the decision-4 signal cannot see the wait it is meant to regulate: the
arbiter's queue is a rendezvous, so `queued_bytes` reads zero while the real wait is in the granted
bytes whose `write_all` is blocked by the peer's Yamux window. This section records the amendment
that closes that gap and what it was observed to do.

**What changed.** `relay_with_policy`'s pump now raises a per-direction `in_flight_bytes` counter
after `FlowHandle::acquire` returns a grant and lowers it (saturating) when that `write_all`
completes, so the counter's meaning is exactly "granted by the arbiter, not yet written by the
relay". The 1 Hz sample reads it beside the arbiter counters, `Counters` carries it, and the
decision-4 estimate divides `queued_bytes + in_flight_bytes` by the interval's granted rate. The
`queued_bytes` metric itself is untouched: it is still the arbiter's own queue length and still
reads zero in this state. With `policy.adaptive` off no controller, no driver and no estimate exist;
the counter is bookkeeping on the relay path and changes no wire byte. No default changed.

**Observed numbers (loopback, `simulated`, this box, one run per line, 2026-11-27).** The harness is
the live-driver test extended with a real relay whose peer drains 512 bytes every 50 ms against a
16 KiB quantum, so a grant is held unwritten across whole sample intervals. The product's own 1 Hz
driver then samples the live counters:

```
POLICY_TP41_INFLIGHT {"queued_bytes":0,"in_flight_bytes":16384,"estimate_ms":1001}
```

| Signal | Value at the same live sample |
|---|---|
| `queued_bytes` (decision 4, unchanged) | **0** — the arbiter granted the request immediately |
| `in_flight_bytes` (amendment) | **16,384** — one full `balanced/1` quantum held in `write_all` |
| estimate with `queued_bytes` only (the old input) | **0 ms** — the 13-run record's result, reproduced |
| estimate with the amendment | **1,001 ms** — 16,384 bytes at the 16 KiB/s grant rate over the 1 s interval |

So the answer to the question decision 4 left open is: **yes, at the relay boundary the signal is
non-zero exactly where `queued_bytes` reads zero**, and it is large (about one second of queueing)
rather than marginal. The test fails if the boundary never reports a held grant, if the arbiter queue
were not zero in that state, or if the live driver never produced a non-zero estimate; the printed
line is the assertion's own evidence, not a hand-computed claim.

**Reverse mutations (each broke the production path, failed the new test, then was restored).**

| Mutation | Observed failure |
|---|---|
| Removed the `in_flight.fetch_add` after `acquire` (the boundary stops knowing a grant exists) | `panicked at src/policy_relay.rs:893: the write boundary never reported granted-but-unwritten bytes while its writer was blocked` |
| Made `counters()` pass `in_flight_bytes: 0` (the sample stops carrying the new signal) | `panicked at src/policy_relay.rs:920: the live sample must turn a held grant into a non-zero estimate; queued_bytes=0, in_flight_bytes=16384, estimates=[(ToPeer, 0ns), (ToPeer, 0ns), …]` |
| Built the controller regardless of the opt-in (`adaptation: Some(…)`) | `panicked at src/policy_relay.rs:819: assertion failed: session.adaptation.is_none()` in `the_live_driver_does_not_run_without_the_opt_in` |

A first attempt at the first mutation left the decrement in place, so the counter wrapped and the
test failed on its bound instead (`one grant is at most one quantum, got 18446744073709535232`). The
decrement was then made saturating, which is also the correct production behaviour if a future edit
reorders the two calls, and the mutation reproduced the intended message above.

**What this does and does not establish.** Verified: the relay path carries the sample behind the
existing `policy.adaptive` opt-in; the live 1 Hz driver reads it and produces a non-zero estimate in
the state where the old input reads zero; the counter is inert without the opt-in. Not established:
anything about a real path. The 16,384 bytes held here are a loopback duplex's backpressure, not a
Yamux window on a real network, the peer's drain rate is chosen by the test, and no benefit is shown
or claimed. The multi-rate decision-12 benchmark (three bottleneck rates, adaptation off vs on,
change spacing and time-to-restore) was **not** re-run with the amended signal in the pass that
wrote this section; it was executed afterwards and is recorded in section 7, and the two-host LAB
window remains TP-4.1's only acceptance criterion.

## 7. The decision-12 benchmark, executed with the amended signal (2026-11-27)

ADR-0056 decision 12 asks for the loop against the static row it starts from, at the plan's three
bottleneck rates, with change spacing, estimate distribution and time-to-restore after the load
stops. The 2026-11-26 record could not produce any of it because every sample read 0 ms (section 3);
with the decision-4 amendment (section 6) the harness now can. This section is the run.

**How it was run.** The existing `adaptive_controller_experiments` harness (same `run_multi_flow`,
same shaped loopback proxy, same three flows) was extended in three places, not replaced: a recovery
phase after the bulk load stops (ping traffic continues so the loop still has a rate), a per-direction
final row and `below_configured` in `AdaptiveReport`, and an `after_stop` block (estimates and changes
in the window, first estimate at or below the ADR's 20 ms up-threshold, first moment both directions
are back on the configured row). `POLICY_ADAPTIVE_PHASE` selects the three-rate matrix or the long
run so each measured pass fits one command. Command, release build:

```
POLICY_ADAPTIVE_PHASE=matrix POLICY_ADAPTIVE_SECONDS=8 POLICY_ADAPTIVE_RECOVERY_SECONDS=10 \
  cargo test --release --features policy-pack --ignored --nocapture adaptive_controller_experiments
```

Two arms per rate — `sched-w262144-q16384-cap65536` (static `balanced/1`) and
`adaptive-w262144-from-balanced/1` — two repeats, three flows, 250 / 500 / 1000 KB/s, 8 s of bulk
load, then 10 s of recovery. One long run follows: 150 s at 250 KB/s with 45 s of recovery, because
the ADR's 30 s dwell outlasts a 10 s window and the row-return question needs the longer one. All
lines carry `evidence: simulated`; the proxy is loopback, not a path.

**Per-rate results (observed, repeat 1 / repeat 2).** Rows are the adaptive arm;

| rate | change | at | estimates | p50 / p95 / p99 / max (ms) | > 50 ms | settled below configured | recovery: calm | recovery: row back |
|---|---|---|---|---|---|---|---|---|
| 250 KB/s | 1: `balanced/1 → gaming/1` (to_peer) | 4,006 / 4,006 ms | 29 / 31 | 0 / 187 / 375 / 375 · 0 / 187 / 187 / 187 | 6 / 6 | yes (`to_peer` `gaming/1`) | 605 / 1,614 ms | not inside 10 s |
| 500 KB/s | 1: `balanced/1 → gaming/1` (to_peer) | 4,006 / 4,006 ms | 34 / 34 | 0 / 93 / 125 / 125 (both) | 3 / 3 | yes | 603 / 605 ms | not inside 10 s |
| 1 MB/s | 1: `balanced/1 → gaming/1` (to_peer) | 5,007 / 5,006 ms | 34 / 34 | 0 / 53 / 53 / 53 (both) | 3 / 3 | yes | 604 / 604 ms | not inside 10 s |
| 250 KB/s, 150 s | 2: down at 4,006 ms, up `gaming/1 → balanced/1` at 156,276 ms | gap 152,270 ms | 282 | 46 / 93 / 125 / 375 | 27 | no (returned) | 811 ms | **5,856 ms** |

Read plainly: **the loop settles below the configured row at every rate** — exactly one rung, at the
third congested sample, and it does not go further even with 150 s of sustained load. The estimate is
non-zero but intermittent: its median is 0 ms because most ticks see no held grant, while p95 is
53-187 ms and the maximum 375 ms. Change spacing in the short runs is a single change, so no gap
exists to measure; the long run gives one gap, 152.3 s, which is the down-step's whole verified
lifetime — **the row returns only after the load stops**, 6.3 s after the bulk tasks stop (the
driver's three calm samples; the 30 s dwell had long since passed). In the 10 s recovery windows the
row did not return, which is the ADR's dwell and persistence working as designed, not a harness gap:
the down-step happens at ~4 s, so a 30 s dwell cannot expire before the 18 s the window ends at.

**Against the static arm (same runs).** Interactive RTT p50 per flow: 250 KB/s 4.72/6.12 ms static
vs 5.02/4.70 ms adaptive; 500 KB/s 4.42/4.46 vs 4.45/4.41; 1 MB/s 4.52/4.52 vs 4.60/4.53. Delivered
bulk bytes: 2.87/2.82 MB (250 KB/s), 4.70/4.65 (500 KB/s), 8.24/8.21 (1 MB/s) — the adaptive arm
delivers 0.4-1.7% fewer bytes in an 8 s window. That is level-to-slightly-worse, **not a benefit**,
and no profile becomes `measured`.

**What the numbers do not settle, recorded instead of smoothed over.** The estimate's numerator is
bytes in flight, which can never exceed the quantum in force, so a down-step caps its own input: some
of the post-step "improvement" is mechanical rather than the path getting calmer. The 150 s run shows
the loop *keeping* the step for 152 s while the load ran — the calm streak never built — so the
mechanism did not cause a spurious keep there, but this pass did not isolate how much of any
improvement is row-dependent. That is the named next mission, not a claim. Everything here is one
host, one loopback proxy and three flows; nothing about a real path, a real Yamux window or a carrier
is measured, and the two-host LAB window remains TP-4.1's only acceptance criterion.

**Reverse mutations (each restored byte-for-byte, `cmp` confirmed).** The new normal test
`the_adaptive_harness_reports_live_estimates_and_recovery` runs the same harness for 3 s + 3 s and
asserts the report it claims:

| Mutation | Observed failure |
|---|---|
| `counters()` in `src/policy_relay.rs` passes `in_flight_bytes: 0` (the production sample stops carrying the signal) | `panicked at src/policy_baseline.rs:2609: the in-flight signal must make the estimate non-zero under load: AdaptiveReport { changes: 0, …, estimates: 10, estimate_p50_ms: 0, estimate_p95_ms: 0, estimate_max_ms: 0, … }` |
| `observe_after_stop` returns `AfterStop::default()` immediately (the harness reports a recovery it never watched) | `panicked at src/policy_baseline.rs:2618: ping traffic after the load stops must still give the loop a rate: AfterStop { estimates: 0, changes: 0, calm_ms: None, configured_ms: None }` |

The first mutation also shows why the number matters: with the field dropped the loop makes **no
change at all** (0 changes, 0 ms), which is exactly the 2026-11-26 result, so the amendment is what
makes the loop observable on this harness rather than a cosmetic addition.
