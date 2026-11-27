# ADR-0048: Policy queues and scheduler (TP-2.1)

**Status:** Accepted and implemented behind `policy-pack`. Decision 4 is decided by the 2026-11-26 experiments: the in-flight bound is the **Yamux per-stream window (256 KiB)**, switchable at runtime; M1/M2/M3 were measured and are not chosen. Decisions 1-3 are now wired: `BAFT_TUNNEL_POLICY_PACK=sched[:<quantum>[:<cap>]]` runs one deficit round-robin arbiter per relay direction with bounded per-flow queues. TP-2.2 is closed (measured `simulated`): no benefit over the window arm in the single-flow or the multi-flow scenario, no regression, arbitration level with the window pump, safe under mid-run flow churn, and decision 6's CPU and idle-latency budget met with the margin stated below. The switch stays off by default and the off path is today's pump.
**Date:** 2026-11-17

**Implementation (2026-11-26, scheduler core):** `src/policy_sched.rs` provides decision 1's bounded per-flow queues and the deficit round-robin rotation, plus an `Arbiter` task per direction that hands each flow its turn. Decisions 3 and 5 (the bounded wait and the off path) are covered by unit tests: a flow is visited at least once per rotation, a quantum bounds one visit, an overflow is refused and counted rather than truncated, a drained flow leaves the rotation, a closed or dropped flow leaves no queue behind, and the arbiter serves contending flows equally. Class weights (decision 2) are untouched and every class is `Balanced`.

**Implementation (2026-11-26, scheduler wired):** `tunnel::relay_stream` selects `relay_with_policy` when `policy_session()` returns a session, i.e. when the switch names `sched`; otherwise it calls `relay_with_idle_timeout` unchanged. Both session loops (forward and server side) create one `PolicySession` per session; each stream registers one flow id in both directions' arbiters. A read is offered to the arbiter and written only after its grant, so exactly one flow per direction writes at a time and the rotation, not task scheduling, decides the order. Queues hold at most one quantum per flow (the relay asks after each granted write, never before), and a request that would exceed `max_queued_bytes` is refused with an error that ends that flow instead of growing memory; the switch rejects a quantum above the cap so that refusal only bites on a misconfiguration. Idle timeouts are per read as before, `close` runs on every exit path (and `Drop` covers cancellation), and `add_to_peer`/`add_from_peer` are fed at the write.

**Implementation (2026-11-26, mechanism):** the experiments below chose the Yamux per-stream window as decision 4's bound. `src/policy_relay.rs` reads `BAFT_TUNNEL_POLICY_PACK` (`window-cap` or `window-cap:<bytes>`, default 256 KiB) and `tunnel::yamux_config` applies that window to both session ends; the feature off or the switch unset keeps the product's 4 MiB and nothing else changes in the data path. The relay byte budget (M1) was built and measured in the harness and is *not* wired: below half the window it cannot be credited, and at half the window it is a 2 MiB bound.
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md), [ADR-0044](ADR-0044-engine-agnostic-policy-interface.md), [ADR-0047](ADR-0047-policy-status-contract.md), research [POLICY_PACK_TP-0.1_RESEARCH.md](POLICY_PACK_TP-0.1_RESEARCH.md) sections 2, 9, issues [#231](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/231), [#232](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/232)

## Context

Baseline (`src/policy_baseline.rs`, simulated, 2.5 MB/s bottleneck, two runs): a 64-byte ping stream has p50 2.3 ms alone and about 1,950-1,970 ms while one bulk stream shares the tunnel. The product's relay hands bytes to Yamux as fast as the stream accepts them; Yamux accepts up to the stream window (4 MiB) and the kernel and pipe buffers hold more. The delay is queueing in front of the bottleneck, which the relay does not control today. Policy can only act where BAFT-TUNNEL owns the data before the wire (ADR-0031 decisions 4 and 13).

## Decision (proposed)

1. **Per-flow queues in the relay, one scheduler per tunnel direction.** Each flow (ADR-0044 `FlowId`) has a bounded byte queue between the user socket and the Yamux stream. The scheduler drains queues with **deficit round robin** per class; a class has a quantum (bytes per round) and a share bound.
2. **Classes and weights from configuration only** (ADR-0031 decision 6). Balanced uses equal quanta and is the fail-safe. Weights for other profiles are TP-3.x and not set here.
3. **Bounded wait for every flow.** No flow waits longer than `MAX_GRANT_DELAY` (ADR-0044) plus one scheduler round for a grant; a starved flow is promoted. The prioritised class has a share cap so Bulk always makes progress.
4. **Bound the in-flight queue below the relay — decided 2026-11-26: the Yamux per-stream window.** The scheduler limits bytes outstanding per class in Yamux and below (the part the baseline shows dominating). The window is the only place that can bound those bytes: a receiver emits a window update only once `max_recv_window / 2` bytes are consumed, so any relay-level counter below that never receives credit, and anything above it is weaker than the window itself. The chosen default is 256 KiB (Yamux's initial window), applied to both session ends by `tunnel::yamux_config` and switchable with `BAFT_TUNNEL_POLICY_PACK=window-cap[:<bytes>]`. The three candidates were built in the harness and measured (results below):
   - M1 (per-flow write budget gated on `send_window()`): works only at `>= window/2` (2 MiB), where it still leaves about 1 s of queue at 2.5 MB/s; below that it stalls the flow forever. Not chosen.
   - M2 (pacing the receiving sink): changes read timing, not the queue in the window; measured equal to Baseline. Not chosen.
   - M3 (smaller socket send buffer / `TCP_NOTSENT_LOWAT`): the bytes queue above the socket in the engine, so it measured equal to Baseline. Not chosen.
5. **Off path equals today.** Pack off means the existing 16 KiB pump, unchanged (ADR-0044 decision 6). All code is behind the `policy-pack` feature until measured.
6. **Overhead budget.** The scheduler must add at most 5% CPU and no more than 1 ms median added latency on an idle tunnel, measured in the harness against Baseline; if it exceeds that, the pack stays off by default and the result is published as is (TP-2.2).

## Experiments (before any default is chosen)

On the harness (simulated), against the same bottleneck and Baseline: implement M1, M2, M3 separately behind test-only switches and report loaded ping p50/p95/p99, bulk throughput and fairness for each, at 2.5 MB/s and at least two other rates, with repeated runs. Include the `aged_session_throttled` scenario to confirm the scheduler does not hide or worsen it. Order of work: harness variants first, then pick one, then write the production scheduler.

## Experiments: results (2026-11-26, evidence level: `simulated`)

`cargo test --release --features policy-pack policy_baseline::tests::mechanism_experiments -- --ignored --nocapture`, with `POLICY_RATE` set to 1,000,000, 2,500,000 and 5,000,000 B/s, `POLICY_REPEATS=2`, `POLICY_SECONDS=6`. Loaded = one 64-byte interactive ping stream beside one bulk uploader through a rate-limited loopback proxy; every arm uses the product's own relay on both ends. Rows are the two repeats' medians, in milliseconds. (Re-run 2026-11-26 with the `sched` arm added; the earlier five-arm run agreed within the spread.)

| Arm | 1 MB/s p50 | 2.5 MB/s p50 | 5 MB/s p50 | 2.5 MB/s p95 |
|---|---|---|---|---|
| Idle (no bulk) | 4.5 | 4.4 | 4.4 | 5.2 |
| Baseline (4 MiB window) | 4,193 | 2,026 | 1,186 | 2,088 |
| M1 credit budget, 2 MiB | 2,246 | 977 | 590 | 1,019 |
| M2 receive pacing, 64 KiB | 4,178 | 2,017 | 1,263 | 2,053 |
| M3 socket backpressure, 32K/16K | 4,162 | 2,094 | 1,325 | 2,163 |
| **Window 256 KiB (chosen)** | **271** | **110** | **61** | **119** |
| Scheduler on that window (decision 5) | 270 | 109 | 62 | 118 |

Structural findings, both asserted as tests so they cannot silently regress:

- `credit_budget_below_the_window_granularity_stalls`: with the product's 4 MiB window, a 64 KiB credit budget stops at 64 KiB and never resumes — a 256 KiB transfer does not finish. Yamux only sends a window update once the receiver consumes `max_recv_window / 2` (2 MiB), so no budget below 2 MiB can ever be credited. This rules M1 out as a way to get below the window.
- `credit_budget_at_half_the_window_completes`: at 2 MiB the flow completes and is bounded at 2 MiB, which is the 1,045 ms row above — better than Baseline, still far from the window arm.
- The window cap is applied at the engine, needs no per-stream wrapper, and also bounds the kernel and pipe buffers that follow it. The `window_cap_experiment` rerun reproduces section 9.2 within about 15%: 256 KiB 114 ms p50, 1 MiB 526 ms, 4 MiB 2,130 ms at 2.5 MB/s.
- `aged_session_under_window_cap`: fresh 545 ms, aged 5,381 ms versus Baseline 544 / 5,370 ms — the bound neither hides nor worsens the aged-session throttle.
- `bulk_bytes_per_s` in the JSON is bytes handed to the engine, not goodput: the shaped bottleneck fixes delivered rate in every arm (by construction), and the window bound lowers the queued amount (2.8 → 2.1 MB/s handed at 2.5 MB/s), not the delivered rate. Real-path goodput under a smaller window (bandwidth-delay product) still needs a delay model and is not claimed.

Hypothesis check (`p95` within 10x idle `p95`): met at 5 MB/s (68 ms against 4.9 ms), not met at 2.5 MB/s (119 ms against 5.2 ms) and not at 1 MB/s (285 ms against 6.9 ms). The remaining queue is the window's worth of Bulk bytes in front of the interactive flow; that is decision 1-3's job (a prioritised class served every round), not the bound's. No profile claim is made.

## Acceptance (hypotheses to confirm or reject, not claims)

- Loaded interactive ping p95 within 10x of idle, instead of about 3 orders of magnitude (Baseline). The window bound alone gets from about 2,090 ms to 119 ms at 2.5 MB/s: a 17x improvement, met at 5 MB/s, **not met** at 2.5 and 1 MB/s. The wired scheduler does not close the remaining gap (p95 118 ms at 2.5 MB/s against a 10x-idle target of about 52 ms): DRR reorders within the window, it does not shrink the window's worth of Bulk bytes queued in front of the interactive flow. A prioritised class that is served before the bulk flow in the same rotation is TP-3.x and is not implemented; the profile claim stays dropped (ADR-0031 decision 10).
- Bulk throughput within 10% of Baseline at the same bottleneck: not measurable in the harness as delivered goodput — the shaped pump fixes the delivered rate for every arm. Only the queued amount changes, which is the bound's purpose; real-path goodput is open.
- No starvation: every class makes progress under sustained load; wait bounded as in decision 3. Met for equal-class flows (`delivery_min_over_max` 1.000 with two and three flows, and after a mid-run close); class weights are not implemented, so the claim is limited to `Balanced`.
- Decision 6's overhead budget (≤5% CPU, ≤1 ms median idle latency against Baseline): met as measured — +1.4% / +4.3% / −8.2% CPU per delivered MB, +0.7% on the unshaped probe, idle p50 −56 / +43 / −73 µs. Margin stated, not certified below the harness's ±12-40% per-run spread (section "Overhead").
- Evidence label `simulated` until a VM or two-host LAB run exists; no LAB claim, no real-path latency claim. All numbers above are harness/loopback.

## Relay wiring (scheduler → relay, implemented 2026-11-26)

Selected by `relay_stream` when `policy_session()` returns a session (`BAFT_TUNNEL_POLICY_PACK=sched`,
`sched:<quantum>` or `sched:<quantum>:<cap>`; the defaults are the 256 KiB window, a 16 KiB quantum and
a 64 KiB per-flow cap). Otherwise `relay_with_idle_timeout` runs and the bytes are today's.

Shape, and how it differs from the plan above:

1. One arbiter task per direction per session (`policy_sched::Arbiter`), created by
   `PolicySession::with_config`. It owns the `Scheduler` and the rotation; it is the only thing that
   decides who writes next.
2. Each flow's read loop keeps the 16 KiB buffer, reads at most one chunk, and calls
   `FlowHandle::acquire(n)`, which returns the same `n` once this flow's turn comes. The flow writes
   that chunk with its own write half. **The plan's separate writer task holding every write half was
   replaced by this grant**: because a grant is exclusive, only one flow writes at a time, and since
   the grant always covers the whole offered chunk there is no second queue and no chunk copy. That
   keeps the hot path at one `acquire` round trip per 16 KiB instead of a hand-off and a copy.
3. Queues: a flow that is waiting holds one chunk, so per-flow queued bytes never exceed the quantum.
   A request that would exceed `max_queued_bytes` is refused (`io::Error`) and the flow's relay ends;
   nothing is buffered past the cap. The switch refuses `quantum > cap` (and a quantum above the
   256 KiB window) so that refusal is only reachable by misconfiguration.
4. Idle timeout stays per flow on the read side, unchanged, as does EOF shutdown of the write half.
5. Fail-safe: class is configuration only (ADR-0031 decision 4); anything unknown is `Balanced`, and
   `Balanced` is one class, so the arbiter is a single rotation. Pack off, feature off or a switch
   value the parser does not know keeps `relay_with_idle_timeout` and today's bytes exactly.

Constraints from the plan, and their tests: a chunk is never reordered or split (the flow id travels
with the bytes; the pump writes exactly the granted length); a write error fails the direction and the
handle's `Drop`/`close` removes the flow from the rotation (`a_dropped_flow_handle_closes_its_queue`,
`a_closed_flow_ends_its_acquire_and_leaves_no_queue`); `close` runs on every exit path, and a flow
whose queue is drained leaves the rotation even without a close; counters are fed at the write
(`relay_stream_uses_the_pack_session_when_one_is_selected` keeps the idle-timeout semantics and checks
that both arbiters hold zero flows afterwards).

### Results (TP-2.2, 2026-11-26, evidence level: `simulated`)

`cargo test --release --features policy-pack policy_baseline::tests::mechanism_experiments -- --ignored
--nocapture` with `POLICY_RATE` = 1,000,000 / 2,500,000 / 5,000,000 B/s, `POLICY_REPEATS=2`,
`POLICY_SECONDS=6`, run again with the `sched` arm in the matrix. Loaded = one 64-byte interactive ping
stream beside one bulk uploader; both ends use the product's relay. The `sched` arm rides the same
256 KiB window as the `window-cap` arm, so the difference is arbitration alone.

| Arm | 1 MB/s p50 | 2.5 MB/s p50 | 5 MB/s p50 | 2.5 MB/s p95 | 2.5 MB/s bulk handed |
|---|---|---|---|---|---|
| Idle | 4.5 | 4.4 | 4.4 | 5.2 | 0 |
| Baseline (4 MiB window) | 4,193 | 2,026 | 1,186 | 2,088 | 3.51 MB/s |
| Window 256 KiB | 271 | 110 | 61 | 119 | 2.21 MB/s |
| **`sched` (window + DRR)** | **270** | **109** | **62** | **118** | **2.22 MB/s** |

p99: sched 300 / 144 / 69 ms against window-cap 283 / 138 / 74 ms, so the scheduler is inside the
run-to-run spread of the window arm at every rate; it is neither a win nor a regression in this
scenario. Idle timeouts and the aged-session throttle are unchanged: `aged_session_under_scheduler`
fresh 538 ms / aged 5,392 ms against Baseline 545 / 5,371 and window-cap 544 / 5,404.

Why there is no benefit here: with one bulk flow and one interactive flow, the window already bounds
the queue to 256 KiB, and DRR can only reorder around 16 KiB chunks within that queue. The scheduler
matters when several flows of different classes share the link (TP-3.x) or when a flow must not
consume the whole window; neither is measured yet, so none is claimed. The cost is one arbiter round
trip per 16 KiB per direction; the next two sections measure that cost and arbitration between
competing flows.

### Overhead (decision 6, 2026-11-26, evidence level: `simulated`)

Two measurements, same machine, same release build.

**Whole-process CPU on the shaped matrix** (`cpu_cost_experiments`): the same scenario and matrix as
the table above, with `getrusage(RUSAGE_SELF)` taken around each run. Both tunnel ends, the loopback
shaper and the test app share the process, so this is an upper bound on the relay; the comparable
figure between arms at one rate is CPU per *delivered* megabyte, because the shaper's work is the
same for every arm and the delivered rate is fixed by construction. Microseconds of CPU per delivered
MB, medians of the two repeats:

| Arm | 1 MB/s | 2.5 MB/s | 5 MB/s |
|---|---|---|---|
| Baseline | 87.4 | 71.3 | 54.5 |
| Window 256 KiB | 62.0 | 68.2 | 51.0 |
| `sched` | 62.9 | 74.4 | 50.0 |

Against Baseline, the budget's reference, the scheduler is +1.4% / +4.3% / −8.2%: inside the 5%
budget at all three rates, worst case +4.3% at 2.5 MB/s. Against the window arm, which isolates
arbitration, it is +1.4% / +9.1% / −2.0%; the +9.1% cell is inside that arm's own repeat spread
(64.3-72.0 µs/MB, 12%), so it is recorded as noise rather than a confirmed regression.

**Relay CPU probe** (`relay_cpu_probe_experiments`): one unshaped loopback session with both ends
in-process, 512 MiB per run, six runs per arm, CPU per megabyte *relayed*. No shaper in the
denominator, so this is the arbiter's own cost per byte:

| Arm | CPU µs per MB relayed (median) | Range over six runs |
|---|---|---|
| Baseline | 6,235 | 5,840-6,602 |
| Window 256 KiB | 7,295 | 6,738-7,813 |
| `sched` | 6,279 | 4,961-10,273 |

The scheduler is +0.7% against Baseline and −14% against the window pump by median, with single-run
spread up to 40%: the arbiter adds no overhead the harness can resolve.

Idle latency, decision 6's second half: on the idle tunnel `sched` p50 is 4,575 / 4,588 / 4,464 µs at
1 / 2.5 / 5 MB/s against Baseline 4,631 / 4,545 / 4,537, i.e. −56 / +43 / −73 µs, and `sched` p95 is at
most +0.72 ms above Baseline. Both halves are inside the budget: **decision 6 is met as measured**,
with the reservation that arm-to-arm spread on this sandbox (±12-40% per repeat) is wider than the
5% threshold, so the numbers bound the overhead but cannot certify a smaller margin. The budget is
about CPU per delivered or relayed byte, not a wall-clock CPU percentage of a real deployment; that
still needs a two-host LAB run.

### Multi-flow and churn (TP-2.2, 2026-11-26, evidence level: `simulated`)

The scheduler's actual claim is arbitration between competing flows, so the harness gained
`run_multi_flow`: `flows` bulk uploaders share the shaped bottleneck, each with its own 64-byte echo
stream for per-flow latency, plus a churn variant that closes flow 0 at half the run. Same rates,
repeats and duration as the matrix (`multi_flow_experiments`). Two flows, medians of two repeats,
per-flow p50 / p95 in ms:

| Rate | Arm | flow A p50 / p95 | flow B p50 / p95 | handed in 6 s |
|---|---|---|---|---|
| 1 MB/s | Baseline | 1,935 / 7,394 | 3,534 / 5,788 | 16.9 MB |
| 1 MB/s | Window | 559 / 602 | 559 / 604 | 6.46 MB |
| 1 MB/s | `sched` | 561 / 575 | 565 / 605 | 6.46 MB |
| 2.5 MB/s | Baseline | 3,400 / 4,233 | 2,669 / 4,233 | 25.3 MB |
| 2.5 MB/s | Window | 252 / 264 | 252 / 256 | 12.2-12.7 MB |
| 2.5 MB/s | `sched` | 247 / 255 | 248 / 255 | 12.7 MB |
| 5 MB/s | Baseline | 2,119 / 2,660 | 1,663 / 2,660 | 33.7 MB |
| 5 MB/s | Window | 142 / 150 | 143 / 162 | 19.6-20.1 MB |
| 5 MB/s | `sched` | 144 / 151 | 144 / 150 | 20.1 MB |

Three flows agree: at 2.5 MB/s p50 385 µs (window) against 378 µs (`sched`), at 5 MB/s 224 against
229, and at 1 MB/s 851 against 849.

Findings, stated plainly:

- **Delivery split is exactly even in every arm** (`delivery_min_over_max` = 1.000): the bottleneck
  serialises the flows and Yamux credits each stream the same window, so the split does not
  discriminate between arms. The scheduler neither starves nor is starved.
- **Per-flow latency is set by the window, not by arbitration**: Baseline's flows queue behind a
  4 MiB window each (1.7-7.4 s), both bounded arms sit at the 256 KiB window (0.14-0.56 s depending
  on rate), and `sched` is within 1% of the window pump at every rate and flow count.
- **Churn is safe**: closing flow 0 at half the run leaves its handed bytes at about half the
  survivor's (5.19 MB against 14.63 MB at 5 MB/s; 3.23 against 8.73 at 2.5), the surviving flows keep
  moving (`continued_after_churn` true in every run), and no arm stalls.
- **Class-based arbitration is not measured**: `SchedulerConfig` carries only a quantum and a cap and
  every flow is `Balanced` (decision 2 leaves weights to TP-3.x), so a multi-class scenario would need
  API that does not exist yet; adding it for the measurement would not have measured shipped code.

### Default decision (re-derived 2026-11-26)

Full body of evidence: single-flow p50/p95/p99, per-MB CPU, multi-flow fairness/latency/throughput,
churn, and the aged-session throttle. The scheduler changes no number outside the run-to-run spread,
and the CPU measurements do not resolve even its arbiter round trip. **The switch stays off by
default.** A default needs a reproducible benefit; there is none in a harness where the per-stream
window already bounds each flow's queue, and the benefit case is class-aware scheduling (TP-3.x),
which is not implemented. This is a measured wash and a measured fit to the CPU budget, not an
absence of measurement.

What would change it: a real bottleneck shared by flows of different classes, and a path with
non-trivial RTT where the window's bandwidth-delay product matters. Both need a delay model and are
open, `simulated`-only questions.

Tests backing the new scenarios: `relay_cpu_probe_relays_every_byte`,
`process_cpu_time_advances_while_work_runs`, `multi_flow_scenario_moves_every_flow`,
`multi_flow_churn_keeps_surviving_flows_moving`, and with the feature
`scheduler_arbitrates_between_multi_flows_evenly` (three flows split the bottleneck; a churned flow
leaves the rotation without stopping the survivors).

## Consequences

- Positive: addresses the measured dominant delay with a bounded, reversible, unprivileged mechanism, and the mechanism that works is a configuration value rather than a hot-path wrapper.
- Cost: a scheduler in the hot path behind its own switch; extra memory per flow (bounded queues: one chunk in flight). The window is per session, not per flow, so the scheduler's fairness is the only per-flow lever until class weights land.
- Positive (measured, limited): the scheduler is wired with no measured benefit and no measured regression in the single-flow, two-flow or three-flow scenarios, meets decision 6's CPU and idle-latency budget, and is safe when a flow closes mid-run; it ships behind `sched`, off by default, with the numbers published rather than a benefit claim.
- Risk: the window bound lowers the bandwidth-delay product a stream can fill on a long path; a smaller window can cost goodput at high RTT. That must be measured with a delay model before any default changes outside the switch.
- Risk (realised): relay-level control cannot reach below the window, so M1 and M2 as specified cannot bound the queue (measured, see above).

## Rejected

- Kernel qdisc/tc (privileged, ADR-0030).
- Prioritising by payload inspection (ADR-0031 rejected alternatives).
- M1 as specified: a per-flow credit budget below the window cannot be credited (it deadlocks), and at `window/2` it bounds 2 MiB, eight times the chosen window.
- M2 as specified: pacing the receiving sink does not change what already sits in the sender's window (measured equal to Baseline).
- M3 alone: the queue is above the socket, so clamping the socket buffer changes nothing (measured equal to Baseline).
- Choosing a mechanism before measuring all three.
