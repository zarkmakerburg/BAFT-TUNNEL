# ADR-0048: Policy queues and scheduler (TP-2.1)

**Status:** Accepted for the queue/scheduler core (implemented 2026-11-26 behind `policy-pack`, not wired into the relay). The in-flight mechanism below (M1/M2/M3) stays undecided until the experiments run.
**Date:** 2026-11-17

**Implementation (2026-11-26):** `src/policy_sched.rs` provides decision 1's bounded per-flow queues and the deficit round-robin rotation. Decisions 3, 4 (the queue bound and the bounded wait) and 5 (off path equals today) are covered by 7 unit tests: a flow is visited at least once per rotation, a quantum bounds one visit, an overflow is refused and counted rather than truncated, and a drained flow leaves the rotation. Nothing calls it yet — the relay still uses the fixed 16 KiB pump, so the data path with the pack off is unchanged. Class weights (decision 2) and the M1/M2/M3 choice are untouched, and no result is claimed.
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md), [ADR-0044](ADR-0044-engine-agnostic-policy-interface.md), [ADR-0047](ADR-0047-policy-status-contract.md), research [POLICY_PACK_TP-0.1_RESEARCH.md](POLICY_PACK_TP-0.1_RESEARCH.md) sections 2, 9, issues [#231](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/231), [#232](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/232)

## Context

Baseline (`src/policy_baseline.rs`, simulated, 2.5 MB/s bottleneck, two runs): a 64-byte ping stream has p50 2.3 ms alone and about 1,950-1,970 ms while one bulk stream shares the tunnel. The product's relay hands bytes to Yamux as fast as the stream accepts them; Yamux accepts up to the stream window (4 MiB) and the kernel and pipe buffers hold more. The delay is queueing in front of the bottleneck, which the relay does not control today. Policy can only act where BAFT-TUNNEL owns the data before the wire (ADR-0031 decisions 4 and 13).

## Decision (proposed)

1. **Per-flow queues in the relay, one scheduler per tunnel direction.** Each flow (ADR-0044 `FlowId`) has a bounded byte queue between the user socket and the Yamux stream. The scheduler drains queues with **deficit round robin** per class; a class has a quantum (bytes per round) and a share bound.
2. **Classes and weights from configuration only** (ADR-0031 decision 6). Balanced uses equal quanta and is the fail-safe. Weights for other profiles are TP-3.x and not set here.
3. **Bounded wait for every flow.** No flow waits longer than `MAX_GRANT_DELAY` (ADR-0044) plus one scheduler round for a grant; a starved flow is promoted. The prioritised class has a share cap so Bulk always makes progress.
4. **Bound the in-flight queue below the relay.** The scheduler limits bytes outstanding per class in Yamux and below (the part the baseline shows dominating). Candidate mechanisms, to be decided by the experiments below, not here:
   - M1: write budget per class gated on the stream's remaining send credit (`send_window()`), so a Bulk stream cannot fill the 4 MiB window;
   - M2: gate reads on the receiving side so window updates (and therefore the sender's in-flight bytes) are paced per class;
   - M3: smaller socket send buffer / `TCP_NOTSENT_LOWAT` on the tunnel socket (unprivileged, research section 4).
5. **Off path equals today.** Pack off means the existing 16 KiB pump, unchanged (ADR-0044 decision 6). All code is behind the `policy-pack` feature until measured.
6. **Overhead budget.** The scheduler must add at most 5% CPU and no more than 1 ms median added latency on an idle tunnel, measured in the harness against Baseline; if it exceeds that, the pack stays off by default and the result is published as is (TP-2.2).

## Experiments (before any default is chosen)

On the harness (simulated), against the same bottleneck and Baseline: implement M1, M2, M3 separately behind test-only switches and report loaded ping p50/p95/p99, bulk throughput and fairness for each, at 2.5 MB/s and at least two other rates, with repeated runs. Include the `aged_session_throttled` scenario to confirm the scheduler does not hide or worsen it. Order of work: harness variants first, then pick one, then write the production scheduler.

## Acceptance (hypotheses to confirm or reject, not claims)

- Loaded interactive ping p95 within 10x of idle, instead of about 3 orders of magnitude (Baseline). If no mechanism gets there, the result is published and the profile claim is dropped (ADR-0031 decision 10).
- Bulk throughput within 10% of Baseline at the same bottleneck.
- No starvation: every class makes progress under sustained load; wait bounded as in decision 3.
- Evidence label `simulated` until a VM or two-host LAB run exists; no LAB claim, no real-path latency claim.

## Consequences

- Positive: addresses the measured dominant delay with a bounded, reversible, unprivileged mechanism.
- Cost: a scheduler in the hot path; extra memory per flow (bounded queues).
- Risk: relay-level control may not reach the wire queue (M1/M2 depend on Yamux accounting; M3 depends on kernel clamps); the result may be "no gain".

## Rejected

- Kernel qdisc/tc (privileged, ADR-0030).
- Prioritising by payload inspection (ADR-0031 rejected alternatives).
- Choosing a mechanism before measuring all three.
