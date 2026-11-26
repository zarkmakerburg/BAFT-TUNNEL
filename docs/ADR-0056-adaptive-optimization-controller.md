# ADR-0056: Adaptive optimisation controller (TP-4.1)

**Status:** Accepted (2026-11-26, TP-4.1 / #238). The design is settled and its pure decision core is shipped behind `policy-pack`, unwired: nothing samples a signal, changes a row or logs yet, adaptation has no opt-in in the config, and there is no benchmark or LAB evidence. Adaptation is off by default and makes no benefit claim; ADR-0048's off-by-default scheduler decision and every `simulated` label are unchanged.
**Date:** 2026-11-26
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decisions 8, 9, 10, 15-19, [ADR-0044](ADR-0044-engine-agnostic-policy-interface.md), [ADR-0047](ADR-0047-policy-status-contract.md), [ADR-0048](ADR-0048-policy-queues-and-scheduler.md) decisions 3-6, plan [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md), issues [#238](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/238), [#239](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/239), [#229](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/229)

## Context

TP-4.1 (#238) asks for "a slow control loop over measured signals (TP-1.1) adjusting only profile-allowed parameters in small bounded steps with minimum dwell time, hysteresis and rate limit; every change logged with the signal that caused it", with acceptance "ADR, benchmark, LAB". ADR-0031 decision 9 fixed the shape of the loop (slow, bounded, dwell, hysteresis, rate limit, logged causes, fail-safe to Balanced) but deliberately left its inputs and numbers open; the plan's risk table names "adaptive loop oscillates" and prescribes exactly those guards.

What exists on `main` today:

- Five accepted profile rows (ADR-0031 decision 15), each a `(window, quantum, cap)` triple, all bounds-checked and resolved into `PolicyRelay` at session start. A session's class is its configured profile; class weights are identity.
- The scheduler's own counters: `ArbiterStats { grants, overflow_refusals, queued_bytes, flows }` and the per-`Scheduler` `grants`/`overflow_refusals`/`total_queued_bytes`, plus `Guarded::violations()` from the interface wrapper (`src/policy.rs`, `src/policy_sched.rs`). They are `pub(crate)` diagnostics; nothing exports them.
- No product RTT, loss or per-packet signal: the metric surface of TP-1.1 (#229) is not implemented (the pack plan records harness-only measurement), and ADR-0047's `policy` status object is still Proposed.
- No runtime reconfiguration path: the window is applied to both Yamux session ends at session creation and cannot change for that session's lifetime, and the profile is resolved once per session.
- No reproducible benefit from the scheduler arm and no measured profile: everything stays off by default and every number is `simulated`; the two-host LAB is suspended.

An ADR that ignored those facts would design a controller nothing can feed. The decision below is constrained to what the code can supply and what ADR-0031 already ratified.

## Decision

1. **The controller chooses rows, it does not compute numbers.** Its only output is one of the five ADR-0031 decision-15 rows. It never interpolates, scales or invents a `window`, `quantum` or `cap`, so every state it can enter is an already-accepted, already-bounds-checked configuration. This is what "profile-allowed parameters" means here: the pack's own table, nothing beside it.
2. **The window is not adaptable; only the scheduler pair is.** `window_bytes` is fixed for a session's lifetime (ADR-0048 decision 4: both Yamux ends assume it at stream creation), so the controller never changes it: the configured profile keeps supplying the window, and adaptation selects only `quantum_bytes` + `max_queued_bytes` from the table. A change therefore needs no session renegotiation, no carrier change and no new parameter. A future window adaptation would need a new ADR and a session-restart design; it is out of scope here.
3. **Fixed ladder, not a search space.** The five rows ordered by their scheduler pair (both numbers strictly increase along it, pinned by a test):

   | Rung | Row | quantum | cap |
   |---|---|---|---|
   | 0 (floor) | `trading/1` | 2 KiB | 8 KiB |
   | 1 | `gaming/1` | 4 KiB | 16 KiB |
   | 2 | `balanced/1` | 16 KiB | 64 KiB |
   | 3 | `streaming/1` | 32 KiB | 128 KiB |
   | 4 | `bulk/1` | 64 KiB | 256 KiB |

   A change moves exactly one rung, and never above the configured row: adaptation may shrink the scheduler pair under sustained queueing delay and restore it as the delay falls, but it cannot widen beyond what the operator configured. `balanced/1` stays the fail-safe target of ADR-0031 decision 2, and a controller that cannot improve simply stays at the configured row.
4. **Inputs are the pack's own counters, on a 1 Hz slow loop.** One fresh sample is `{queue_delay, overflow_refusals, violations}`:
   - `queue_delay` — the tunnel-internal queueing-delay **estimate** for the direction, derived as queued bytes divided by the recent delivered rate (the arbiter's `queued_bytes` and granted bytes over the last interval). It is labelled `estimated`, not `measured`, in ADR-0024 terms and makes no claim about wire RTT or the internet path.
   - `overflow_refusals` — the scheduler's cumulative refusal counter; an increase means a queue hit its cap and a flow was refused, which is a guard condition, not a tuning signal.
   - `violations` — `Guarded`'s cumulative interface-violation counter; an increase is a fail-safe trigger.
   - Freshness is part of the signal: a tick with no fresh sample is a missed sample, not a zero.

   The controller uses no RTT, loss, retransmission, port, protocol or payload signal, and no inference of any kind (ADR-0031 decisions 6 and 8). A wire RTT source would need a product metric surface (#229) and is recorded as a limit, not assumed.
5. **Objective and direction.** The loop minimises the queueing-delay estimate inside the tunnel's own control by shrinking the scheduler pair while the estimate stays high, and restores the configured row once it stays low. It claims nothing about throughput, goodput, loss or path latency; the measured cost/benefit of every row remains the TP-3.x evidence question, unchanged by adaptation.
6. **Hysteresis and timing (provisional design values, not measurements).**

   | Knob | Value | Why |
   |---|---|---|
   | sample period | 1 s | The loop must be far slower than the relay; one sample per second is ~1/16 of the dwell and costs one counter read. |
   | persistence | 3 consecutive samples | Debounce against a single burst; a step needs sustained pressure. |
   | step-down threshold | queueing estimate > 50 ms | ADR-0048's acceptance uses "loaded p95 within 10x idle", which at 2.5 MB/s (idle p50 about 5 ms) is about 50 ms; the plan's risk table asks for hysteresis rather than a single trigger. |
   | step-up threshold | estimate < 20 ms | 40% of the step-down threshold; the 30 ms gap is the deadband that prevents flapping around one value. |
   | minimum dwell | 30 s | Longer than any verification window's half; a state is held long enough to be observed. |
   | rate limit | 3 changes per 300 s | Bounds the worst case even if dwell and verification are satisfied repeatedly. |
   | verification window | 60 s after a down-step | The step must show its effect before another is allowed; 60 s is 60 samples, ample for the 1 s loop. |
   | minimum improvement | 10% of the pre-step estimate | A step that cannot move the estimate by more than the harness's per-run spread is treated as ineffective (the TP-2.2/ADR-0048 runs report ±12-40% spread, so 10% is a floor, not a significance claim). |
   | bad-row cooldown | 600 s | A row that failed verification is not retried immediately. |
   | guard suspension | 300 s | After a violation or overflow, adaptation is suspended for five minutes while the configured row runs. |
   | staleness | 3 missed samples | No fresh data means no adaptation; the configured row is restored. |

   These values are the design's starting point. The benchmark (decision 12) may amend them through an ADR amendment; until then they are neither evidence nor a claim.
7. **Bounded, verified, reversible steps.** A down-step is taken on the third consecutive sample above the step-down threshold if dwell and rate limit allow; it records the pre-step estimate as its baseline. At the end of the verification window the step is kept only if the estimate improved by at least 10%; otherwise the controller reverts exactly that step (to the row that was already verified, not all the way to the configured row), marks the failed row for the cooldown and logs the cause. A second step cannot start while one is under verification.
8. **Fail-safe.** These are not adapted around:
   - an increase in `violations` or in `overflow_refusals` restores the configured row immediately, clears any pending verification and suspends adaptation for 300 s;
   - three consecutive missed samples restore the configured row and adaptation resumes only once fresh samples have rebuilt a full persistence streak;
   - the configured row is the anchor: if the configured profile is `balanced/1`, the restore target is already ADR-0031's fail-safe; if it is another row, its own row is the operator's stated intent and the anchor. Falling below `trading/1` is impossible (the floor), so there is always a bounded state to fall back to.
9. **Every change is one log line with its cause.** The controller returns a change record `{at, from, to, cause, queue_delay}` rendered as `policy adaptive: <from> -> <to> cause=<cause> queue_delay_ms=<n>`; causes are the closed set `queue_delay_high`, `queue_delay_low`, `no_improvement`, `guard_violation`, `overflow`, `stale_signals`. There is no per-sample logging, and a change with no attributable signal cannot be produced.
10. **How it stays off by default.** Three independent gates, in addition to nothing being wired:
    - the `policy-pack` cargo feature (off by default) owns the code;
    - `BAFT_TUNNEL_POLICY_PACK` must enable the pack at runtime, exactly as for the window and scheduler today;
    - the tunnel config must opt in with a new boolean `policy.adaptive` (default absent = `false`), added by the wiring step, not by this ADR's slice. With the field absent or false the session runs its configured row end to end, and the data path is byte-identical to today's. With the pack off the field is inert and logged as such; it does not switch the pack on.

    Adaptation never changes the window, never changes the class label, never enables a profile and never widens beyond the configured row. ADR-0048's decision that the scheduler switch stays off (no reproducible benefit) and the `simulated` label of every existing record are untouched; no profile becomes "ready" through adaptation.
11. **Status contract is reserved, not extended here.** When ADR-0047's `policy` object ships it will need the adaptive view (`state`, configured row, current row, last change and cause). ADR-0047 stays Proposed; no endpoint, field or panel card is added by this ADR and none is claimed.
12. **Benchmark plan (TP-4.1's second acceptance item), not executed here.** With the wiring in place, the harness compares adaptation off against on at fixed seeds and at least three bottleneck rates, on the ADR-0048 matrix: the queueing-delay estimate over time, loaded ping p50/p95/p99, the sender-side handoff count (never called delivered goodput), CPU per delivered MB, number and spacing of changes over a 10-minute run, time-to-restore after the load stops, and every rollback/suspension path. Evidence is `simulated`/CI only; the result must state whether the loop is level with the static configured row, better, or worse, and a cost with no benefit is a reason to leave the opt-in off by default. A 10-minute run is the plan's ceiling; the 60-minute profile run stays a TP-3.x/LAB item.
13. **LAB (TP-4.1's third acceptance item) is suspended and unmeasured.** The two-host window is the only place the loop's real behaviour (carrier RTT, path change, impairment) can be observed; nothing in this ADR or its slice is LAB evidence, and TP-3.2-TP-3.5's LAB criteria remain open as they were.

## Testable acceptance for the slice shipped with this ADR

The pure core `src/policy_adaptive.rs` (feature `policy-pack`, not called from the relay) must satisfy, by unit tests that run in both feature configurations of `cargo test --all-targets`:

1. The ladder is exactly the five ratified rows, in strictly increasing quantum and cap order.
2. A fresh controller starts at the configured row and a calm stream (estimate below the step-up threshold) never changes it.
3. Three consecutive samples above the step-down threshold produce exactly one change, one rung down, with `queue_delay_high` and the sampled estimate recorded.
4. Every change moves exactly one rung and never above the configured row; the floor row never steps below itself; the deadband value between the two thresholds never changes anything.
5. Changes are never closer than the dwell, and no more than three occur inside a 300 s window.
6. A down-step that improves the estimate by at least 10% inside 60 s is kept and does not itself produce a change; a step that does not improve is reverted to the previously verified row with `no_improvement`, and its row is not retried inside the 600 s cooldown.
7. Calm samples restore towards the configured row one rung at a time and stop there.
8. Three missed samples restore the configured row with `stale_signals`.
9. A `violations` or `overflow_refusals` increase restores the configured row with its cause and suspends further changes for 300 s; adaptation resumes afterwards within the dwell and rate limit.
10. The rendered log line carries the from/to rows, the cause and the triggering estimate.

## What shipped with this ADR (implementation note, 2026-11-26)

- `src/policy_adaptive.rs`: the constants above, `LADDER`, `Signals`, `Cause`, `Change` (`Display`), the private `Pending` verification record and `Controller::{new, configured, current, update}`. Pure and deterministic: time is a `Duration` argument and every rule is a function of the sample sequence, so tests need no clock, task, socket or I/O. `#![allow(dead_code)]` for the same reason `policy.rs` carries it at TP-2.0: the relay does not call the module yet.
- `mod policy_adaptive` is declared behind `#[cfg(feature = "policy-pack")]` in `src/main.rs`; the default build does not compile it.
- Fifteen unit tests in the module cover acceptance 1-10 and the revert/cooldown, verification, suspension and formatting edges beyond them.
- **Not in this slice:** signal sampling, the `policy.adaptive` config field, any relay or scheduler call, any timer or task, logging to the tunnel log, the status view, the benchmark, and any LAB measurement. The default data path is unchanged: the module is never invoked, and an unset switch keeps today's bytes exactly.

## What TP-4.1 still needs

One step: **the wiring and benchmark slice** — add the `policy.adaptive` opt-in, derive the sample from the arbiter's counters and the `Guarded` wrapper, apply a change to the direction's scheduler pair, emit the change line, and run the decision-12 benchmark into a `simulated` evidence record. TP-4.1 then stays open only on its two-host LAB item, like TP-3.2-TP-3.5. TP-4.2 (#239) — fail-safe, rollback and multi-condition tests — follows, and its rollback matrix is already specified by decisions 7 and 8 here.

## Rejected

- **Continuous numeric tuning (PID-like or proportional steps) on `window`/`quantum`/`cap`.** It would create values no profile has accepted or measured, needs new bound tests, and cannot be exercised by the per-tuple evidence process. Choosing among ratified rows is the smallest rule that still adapts.
- **Changing the window mid-session.** Yamux assumes the window at stream creation (ADR-0048 decision 4); pretending otherwise would either be inert or break the carrier. Window adaptation needs its own decision and session-restart design.
- **Adapting on wire RTT or loss.** No product signal exists (#229 is not implemented), and the pack cannot claim path effects (ADR-0031 decision 4). Inventing a probe here would be a second metrics design inside a controller item.
- **Adapting upwards past the configured row.** An operator who configured `bulk/1` asked for throughput; automatic widening would silently override configuration and could only increase the queueing the loop is meant to bound.
- **Always-on adaptation, or adaptation as the default.** ADR-0031 decisions 2 and 9 make the pack optional and conservative; the scheduler arm has no reproducible benefit to justify a default (ADR-0048), so adaptation needs an explicit opt-in and stays behind the existing gates.
- **A per-sample log or a metric export for the loop.** One line per change, with its cause, is what the issue asks for; a metrics surface is TP-1.1's ADR, not this one.
