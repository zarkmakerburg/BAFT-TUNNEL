# ADR-0048: Policy queues and scheduler (TP-2.1)

**Status:** Accepted. Decisions 1-3 and 5 are implemented behind `policy-pack`. Decision 4 is decided by the 2026-11-26 experiments: the in-flight bound is the **Yamux per-stream window (256 KiB)**, switchable at runtime; M1/M2/M3 were measured and are not chosen. The queue/scheduler core is still not called by the relay.
**Date:** 2026-11-17

**Implementation (2026-11-26):** `src/policy_sched.rs` provides decision 1's bounded per-flow queues and the deficit round-robin rotation. Decisions 3 and 5 (the bounded wait and the off path) are covered by 7 unit tests: a flow is visited at least once per rotation, a quantum bounds one visit, an overflow is refused and counted rather than truncated, and a drained flow leaves the rotation. Nothing calls it yet — the relay still uses the fixed 16 KiB pump, so the data path with the pack off is unchanged. Class weights (decision 2) are untouched.

**Implementation (2026-11-26, mechanism and wiring):** the experiments below chose the Yamux per-stream window as decision 4's bound. `src/policy_relay.rs` reads `BAFT_TUNNEL_POLICY_PACK` (`window-cap` or `window-cap:<bytes>`, default 256 KiB) and `tunnel::yamux_config` applies that window to both session ends; the feature off or the switch unset keeps the product's 4 MiB and nothing else changes in the data path. The relay byte budget (M1) was built and measured in the harness and is *not* wired: below half the window it cannot be credited, and at half the window it is a 2 MiB bound.
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

`cargo test --release --features policy-pack policy_baseline::tests::mechanism_experiments -- --ignored --nocapture`, with `POLICY_RATE` set to 1,000,000, 2,500,000 and 5,000,000 B/s, `POLICY_REPEATS=2`, `POLICY_SECONDS=6`. Loaded = one 64-byte interactive ping stream beside one bulk uploader through a rate-limited loopback proxy; every arm uses the product's own relay on both ends. Rows are the two repeats' medians, in milliseconds.

| Arm | 1 MB/s p50 | 2.5 MB/s p50 | 5 MB/s p50 | 2.5 MB/s p95 |
|---|---|---|---|---|
| Idle (no bulk) | 4.5 | 4.8 | 4.7 | 8.0 |
| Baseline (4 MiB window) | 4,200 | 2,160 | 1,330 | 2,170 |
| M1 credit budget, 2 MiB | 2,260 | 1,045 | 623 | 1,071 |
| M2 receive pacing, 64 KiB | 4,210 | 2,161 | 1,349 | 2,271 |
| M3 socket backpressure, 32K/16K | 4,170 | 2,217 | 1,402 | 2,373 |
| **Window 256 KiB (chosen)** | **269** | **116** | **62** | **133** |

Structural findings, both asserted as tests so they cannot silently regress:

- `credit_budget_below_the_window_granularity_stalls`: with the product's 4 MiB window, a 64 KiB credit budget stops at 64 KiB and never resumes — a 256 KiB transfer does not finish. Yamux only sends a window update once the receiver consumes `max_recv_window / 2` (2 MiB), so no budget below 2 MiB can ever be credited. This rules M1 out as a way to get below the window.
- `credit_budget_at_half_the_window_completes`: at 2 MiB the flow completes and is bounded at 2 MiB, which is the 1,045 ms row above — better than Baseline, still far from the window arm.
- The window cap is applied at the engine, needs no per-stream wrapper, and also bounds the kernel and pipe buffers that follow it. The `window_cap_experiment` rerun reproduces section 9.2 within about 15%: 256 KiB 114 ms p50, 1 MiB 526 ms, 4 MiB 2,130 ms at 2.5 MB/s.
- `aged_session_under_window_cap`: fresh 545 ms, aged 5,381 ms versus Baseline 544 / 5,370 ms — the bound neither hides nor worsens the aged-session throttle.
- `bulk_bytes_per_s` in the JSON is bytes handed to the engine, not goodput: the shaped bottleneck fixes delivered rate in every arm (by construction), and the window bound lowers the queued amount (2.8 → 2.1 MB/s handed at 2.5 MB/s), not the delivered rate. Real-path goodput under a smaller window (bandwidth-delay product) still needs a delay model and is not claimed.

Hypothesis check (`p95` within 10x idle `p95`): met at 5 MB/s (72 ms against 8.2 ms), not met at 2.5 MB/s (133 ms against 8.0 ms) and not at 1 MB/s (274 ms against 6.0 ms). The remaining queue is the window's worth of Bulk bytes in front of the interactive flow; that is decision 1-3's job (a prioritised class served every round), not the bound's. No profile claim is made.

## Acceptance (hypotheses to confirm or reject, not claims)

- Loaded interactive ping p95 within 10x of idle, instead of about 3 orders of magnitude (Baseline). The window bound alone gets from about 2,170 ms to 133 ms at 2.5 MB/s: a 16x improvement, met at 5 MB/s, **not met** at 2.5 and 1 MB/s. The remaining gap needs the per-class scheduler (decisions 1-3), so the profile claim stays dropped until that is wired and measured (ADR-0031 decision 10).
- Bulk throughput within 10% of Baseline at the same bottleneck: not measurable in the harness as delivered goodput — the shaped pump fixes the delivered rate for every arm. Only the queued amount changes, which is the bound's purpose; real-path goodput is open.
- No starvation: every class makes progress under sustained load; wait bounded as in decision 3. Unchanged — the bound applies to every flow equally today.
- Evidence label `simulated` until a VM or two-host LAB run exists; no LAB claim, no real-path latency claim. All numbers above are harness/loopback.

## Relay wiring plan (scheduler → relay, next change)

The mechanism is decided and the one wired lever is the window. The scheduler (`src/policy_sched.rs`) still has no caller. Its connection to the relay is specified here so the next change is mechanical and measurable.

Today `tunnel::relay_with_idle_timeout` splits both sides and runs two independent `pump_with_idle_timeout` loops, each reading at most 16 KiB and writing it straight through. No ordering exists between flows or classes: the interactive flow waits behind whatever the bulk flow already handed to Yamux — now at most one window (256 KiB) instead of 4 MiB.

Target shape, one writer per direction:

1. `relay_with_policy(peer, local, idle, policy)` is selected where the pack is on; the direction that writes to the peer stops writing inline.
2. Each flow's read loop keeps its 16 KiB buffer and offers at most `quantum_bytes` (16 KiB) to the shared scheduler through a bounded channel of `max_queued_bytes` (64 KiB). A full channel applies backpressure to that flow's socket instead of growing a queue — the same refuse-never-truncate rule the unit tests assert.
3. One writer task per direction owns the `Scheduler` and the peer write half. It takes the next granted flow (`next_grant`), writes that chunk and releases the credit. Deficit round-robin guarantees every registered flow is visited once per rotation, so a one-chunk flow's wait is bounded by `flows_in_rotation × quantum / rate`.
4. The idle timeout stays per flow on the read side, unchanged: a flow whose peer goes quiet still fails its direction after `idle`.
5. Fail-safe: the class comes only from explicit configuration (ADR-0031 decision 4); anything unknown or unset is `Balanced`, and `Balanced` is a single class, so the writer behaves like today's pump. Pack off, feature off or switch unset keeps `relay_with_idle_timeout` and today's bytes exactly.

Constraints that the wiring change must satisfy:

- a chunk is never reordered or split across flows: the flow id travels with the bytes;
- a write error fails the direction, closes the flow and wakes every waiting flow so none is stranded;
- `close(flow)` runs on every exit path, cancellation included, or the rotation keeps visiting a dead flow;
- `add_to_peer` / `add_from_peer` (ADR-0024) are fed at the write, not at the enqueue, so they keep meaning "bytes handed to the peer".

Measurement for that change (TP-2.2, evidence `simulated` until a LAB window exists): the `mechanism_experiments` scenario with a prioritised interactive class, reporting loaded ping p50/p95/p99, fairness across two equal bulk flows, the writer path's per-MB cost against `relay_with_idle_timeout`, and the idle timeout unchanged. Acceptance is the ADR-0048 hypothesis (p95 within 10x idle) or a published negative result.

## Consequences

- Positive: addresses the measured dominant delay with a bounded, reversible, unprivileged mechanism, and the mechanism that works is a configuration value rather than a hot-path wrapper.
- Cost: a scheduler in the hot path; extra memory per flow (bounded queues). The window is per session, not per flow, until decision 1-3 land.
- Risk: the window bound lowers the bandwidth-delay product a stream can fill on a long path; a smaller window can cost goodput at high RTT. That must be measured with a delay model before any default changes outside the switch.
- Risk (realised): relay-level control cannot reach below the window, so M1 and M2 as specified cannot bound the queue (measured, see above).

## Rejected

- Kernel qdisc/tc (privileged, ADR-0030).
- Prioritising by payload inspection (ADR-0031 rejected alternatives).
- M1 as specified: a per-flow credit budget below the window cannot be credited (it deadlocks), and at `window/2` it bounds 2 MiB, eight times the chosen window.
- M2 as specified: pacing the receiving sink does not change what already sits in the sender's window (measured equal to Baseline).
- M3 alone: the queue is above the socket, so clamping the socket buffer changes nothing (measured equal to Baseline).
- Choosing a mechanism before measuring all three.
