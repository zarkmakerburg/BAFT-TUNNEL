# ADR-0031: Adaptive Traffic Policy Pack

**Status:** Proposed. Nothing is implemented.
**Date:** 2026-11-15
**Related:** [ADR-0018](ADR-0018-engine-framework.md), [ADR-0024](ADR-0024-p1-operations-evidence.md), [ADR-0030](ADR-0030-read-only-firewall-visibility.md), plan [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md), epic [#226](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/226)

## Context

Operators want transport behaviour tuned to the kind of use (interactive/gaming, trading, streaming, bulk). BAFT-TUNNEL carries traffic between servers; user management, UUIDs, subscriptions, VPN protocols and quotas stay with Xray and its panels. No earlier ADR or roadmap item covers traffic policy (checked 2026-11-15: ADR-0001..0030, roadmap, no open duplicate issues).

## Decision (proposed)

1. **One shared transport engine, many policies.** No separate engine per profile. Profiles are versioned parameter sets (Balanced, Gaming, Trading, Streaming, Bulk) consumed by one Traffic Policy Engine.
2. **Optional add-on.** The core never depends on it. Off by default; when off, the data path is the same as today. Switchable at runtime; Balanced is the default profile and the fail-safe target.
3. **Userspace only.** Policy lives inside the tunnel's own queues and scheduling. No `CAP_NET_ADMIN`, no kernel qdisc/tc, no firewall or sysctl changes (ADR-0030, roadmap constraint 8).
4. **Scope of effect.** Only delay, jitter, throughput and fairness inside BAFT-TUNNEL's control (queueing delay, scheduling, buffer sizing). No claim of reducing internet path latency, geographic distance or destination latency.
5. **Fairness is mandatory.** Every profile keeps a bounded wait for all flows (no starvation) and a bounded share for the prioritised class.
6. **Classification without inspection.** Flow class comes from explicit configuration (per tunnel/listener mapping to a profile). No deep packet inspection and no traffic guessing in the first version.
7. **Engine coverage.** Rust engines (`tcp-tls`, `noise-tcp`) first. The BAFT Go engine is a separate process; policy cannot reach inside it, so it is out of scope unless a later ADR decides otherwise. Direct/Reverse detection stays before engine selection; this pack does not change it.
8. **Measured, not claimed.** Metrics carry unit, interval, provenance and freshness. Measured values and estimates are separate fields. Packet loss over a reliable carrier is only a proxy and is labelled so.
9. **Adaptive control is conservative.** Slow loop, bounded steps, minimum dwell time, hysteresis, rate limit, logged causes, automatic fall-back to the last known-good or Balanced settings on stale metrics or regression.
10. **Evidence gate.** A profile is never "ready" because it exists. Each needs measured results vs Balanced and Baseline, with evidence level stated (CI-simulated, VM, real two-host LAB, never Production).

## Amendment 2026-11-15: UDP carrier and other engines (owner follow-up)

11. **UDP/datagram carrier is a prerequisite for Gaming/Trading benefit claims, and a separate work item** ([#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244)). It needs its own ADR, independent security audit, acceptance matrix and real two-host LAB. It is not part of this pack. Order: after Stable 0.2.0; not needed for the policy engine (Phase 2), Balanced, Streaming or Bulk, which run on TCP/Yamux; it must land before Gaming/Trading acceptance (TP-3.2, TP-3.3). Until then those two profiles may be built and measured on TCP but make no benefit claim that depends on avoiding head-of-line blocking.
12. **Engine-agnostic policy interface** ([#245](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/245), first item of Phase 2). A stable, versioned interface; each engine/carrier declares capabilities (priority enforcement, write pacing, RTT/queue observation, datagram support). The interface must be carrier-agnostic so a datagram path can be added later without redesign.
13. **What the policy layer can and cannot control.** It controls admission, queueing, scheduling and pacing of data before it is handed to an engine, plus backpressure. For an engine with its own congestion control or buffering (the BAFT engine, a separate binary per ADR-0019; a future QUIC engine) it cannot control internal queues, congestion window, retransmission, FEC or wire behaviour. Effect there depends on how small and honest the engine's inner buffering is and must be measured; it may be nil. The BAFT engine stays out of scope until a later ADR; this does not change ADR-0019.
14. **Per-engine acceptance rule.** A profile claim belongs to one tuple (engine, carrier, profile). No engine inherits another engine's results. Each tuple needs its own benchmarks against Balanced and Baseline with an evidence level. A tuple without evidence is shown Unsupported/Unknown in API, CLI and panel, never "ready".

## Known limits to verify in Phase 0 (not yet researched)

- A single TCP carrier can still cause head-of-line blocking; the Gaming/Trading benefit may be limited without a datagram path (QUIC/UDP is not implemented, roadmap Phase 5). See decision 11.
- Kernel socket buffers contribute to bufferbloat and may be adjustable only partially without privileges.
- Impairment tooling for benchmarks without `CAP_NET_ADMIN` on LAB hosts needs a design (CI namespaces or approved LAB method).

## Delivery order and priority

Phases 0-5 as in the plan. The pack starts only after Stable 0.2.0 and must not delay core work (roadmap queue, Phase 6 hardening). LAB use is serialized with the 0.2.0 acceptance work. If Phase 0 shows that order or dependencies must change, this ADR is amended with the technical reason.

## Consequences

- Positive: a path to measurable, per-use-case tuning without new engines or privileged access.
- Cost: a policy layer in the hot path needs an overhead budget and fairness tests (TP-2.2).
- Risk: benefit may be small on TCP-only carriers; the evidence gate allows dropping a profile that does not beat Balanced.

## Rejected alternatives

- Separate engines per use case (duplication, larger attack surface).
- Kernel tc/qdisc shaping (needs `CAP_NET_ADMIN`, rejected by owner in ADR-0030).
- Traffic classification by payload inspection (privacy, complexity, fragile).
