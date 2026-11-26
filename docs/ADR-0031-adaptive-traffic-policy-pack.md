# ADR-0031: Adaptive Traffic Policy Pack

**Status:** Accepted (2026-11-26, TP-0.2 / #228). The runtime stays gated: everything is behind the `policy-pack` cargo feature and the `BAFT_TUNNEL_POLICY_PACK` runtime switch, both off by default. Acceptance fixes the design and the taxonomy; it accepts no profile as measured and claims no benefit.
**Date:** 2026-11-15
**Related:** [ADR-0018](ADR-0018-engine-framework.md), [ADR-0024](ADR-0024-p1-operations-evidence.md), [ADR-0030](ADR-0030-read-only-firewall-visibility.md), plan [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md), epic [#226](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/226)

## Context

Operators want transport behaviour tuned to the kind of use (interactive/gaming, trading, streaming, bulk). BAFT-TUNNEL carries traffic between servers; user management, UUIDs, subscriptions, VPN protocols and quotas stay with Xray and its panels. No earlier ADR or roadmap item covers traffic policy (checked 2026-11-15: ADR-0001..0030, roadmap, no open duplicate issues).

## Decision

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

## Ratification 2026-11-26 (TP-0.2 / #228): profile taxonomy, traffic class, engine scope

TP-0.2 settles the three questions this ADR left open — the profile taxonomy, the traffic-class
definition and the engine scope — and freezes them as schema version `1`. The measurement that
closed TP-2.2 (the scheduler arm level with the window arm, no reproducible benefit, switch off)
is not re-opened: accepting this ADR turns nothing on. Every profile below is accepted as a
**definition, not a result**; decision 10's evidence gate still decides whether one may ever be
shown as measured.

15. **Profile taxonomy (frozen, schema version `1`).** Five profiles, lowercase ids, each a
    versioned set of transport parameters resolved into knobs the pack already has: the
    per-session Yamux stream window (`window_bytes`, ADR-0048 decision 4), and the deficit
    round-robin quantum (`quantum_bytes`) and per-flow queue cap (`max_queued_bytes`) of
    `SchedulerConfig`, plus the profile's bounded-wait limit (`max_grant_delay_ms`, never above
    the global `MAX_GRANT_DELAY` of 250 ms). No profile introduces a new transport, engine or
    kernel mechanism.

    | Profile | id | window_bytes | quantum_bytes | max_queued_bytes | max_grant_delay_ms | What it changes |
    |---|---|---|---|---|---|---|
    | Balanced (default, fail-safe) | `balanced/1` | 262144 (256 KiB) | 16384 (16 KiB) | 65536 (64 KiB) | 250 | Identity: exactly the pack-on parameters TP-2.1/TP-2.2 measured, and the target every profile falls back to |
    | Gaming | `gaming/1` | 131072 (128 KiB) | 4096 (4 KiB) | 32768 (32 KiB) | 50 | Half the in-flight bytes per stream and a quarter of the quantum: lower tunnel-internal queueing delay for low-rate interactive flows, at the cost of per-stream throughput |
    | Trading | `trading/1` | 65536 (64 KiB) | 2048 (2 KiB) | 16384 (16 KiB) | 25 | The narrowest window/quantum pair the parser accepts while still holding several quanta in flight: lowest tunnel-internal delay, throughput deliberately sacrificed |
    | Streaming | `streaming/1` | 524288 (512 KiB) | 32768 (32 KiB) | 131072 (128 KiB) | 250 | Twice Balanced's window and quantum: sustained medium-bitrate delivery without a large standing buffer |
    | Bulk Transfer | `bulk/1` | 2097152 (2 MiB) | 65536 (64 KiB) | 262144 (256 KiB) | 250 | Four times Balanced's window: throughput for bulk transfers, still half of today's unmanaged 4 MiB window, latency accepted |

    All five rows satisfy the relay parser's existing bounds (`quantum_bytes >= 1024`,
    `quantum_bytes <= window_bytes`, `max_queued_bytes >= quantum_bytes`,
    `max_queued_bytes <= 1 MiB`), so TP-3.x implements them through `SchedulerConfig` as it exists
    and adds no scheduler type. Why these values: Balanced is not a new choice but the measured
    pack default; Gaming and Trading only shrink what the tunnel itself controls (in-flight bytes,
    queueing delay), which is the only effect decision 4 permits; Streaming and Bulk only widen
    in-flight bytes, and since ADR-0048 decision 4 bounded the measured arm at 256 KiB, each wider
    value is a hypothesis to be measured per tuple, not a throughput promise.

16. **Traffic-class definition.** A flow's class is the profile configured for its tunnel, and
    nothing else. TP-3.1 adds exactly one config field, `policy.profile`, to the tunnel's JSON
    config (the one that already carries `engine`, `listen`/`ingress` and `target`), holding one of
    the five lowercase ids; missing or unknown means `balanced/1`. Every Yamux stream of that
    tunnel's session carries that class. There is no payload inspection, no inference from port,
    protocol or volume, and no guessing (decision 6). The `Class` enum in `src/policy.rs` already
    has exactly these five variants, so the class vocabulary is the code's vocabulary.

    Because one tunnel today has one listener/ingress and one target, a session is single-class.
    Class-based arbitration weights therefore stay **identity (every class equal)** in schema
    version `1`: `SchedulerConfig` keeps one quantum and one cap, and the arbiter keeps rotating
    equally. Per-class weights are reserved for a session that genuinely mixes classes, which
    needs a per-service ingress mapping the configuration cannot express today and no work item
    covers; adding it here would create an API nothing can measure. This is also why TP-2.2
    measured no arbitration benefit — with one class per session the arbiter has nothing to
    arbitrate. A later ADR may add multi-service ingress together with the weight table.

17. **Engine scope (frozen).** Profiles apply to the Rust engines that run through the relay's
    Yamux session: `tcp-tls` and `noise-tcp`, on the TCP carrier only. The pack sits in the relay
    above the engine (ADR-0044 decision 1), so a profile acts on the session window, the relay's
    chunking and the per-flow queues — never on engine internals.
    - `baft` (separate Go process, ADR-0019): out of scope. Its capability fields stay
      `Unsupported`/`Unknown` (ADR-0044 decision 4) and its tuple claim stays `unknown`, never
      "ready"; ADR-0019 does not change.
    - UDP/datagram carrier ([#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244),
      ADR-0046, still Proposed): outside this acceptance. Gaming and Trading may be built and
      measured on TCP, but their claims are limited to tunnel-internal queueing delay; no
      head-of-line-blocking benefit may be claimed before that carrier lands and is accepted
      (decision 11).
    - Direct/Reverse detection keeps running before engine selection (ADR-0021); this pack
      changes neither.

18. **What TP-3.x implements, and its first step.** The first concrete step is **TP-3.1 (#233),
    the profile schema plus Balanced**: a versioned profile object parsed from the tunnel config,
    validated against the table in decision 15, resolved into `SchedulerConfig` and the session
    window, with `balanced/1` as the identity profile and any invalid or unknown value falling
    back to Balanced with a logged cause (decision 9). TP-3.2 Gaming, TP-3.3 Trading, TP-3.4
    Streaming and TP-3.5 Bulk each add one row of the same table plus their per-tuple evidence
    record; none may change the mechanics, only the parameters.

19. **Testable acceptance for this schema (used by TP-3.x).** For every profile row: the config
    round-trips unchanged; the resolved `SchedulerConfig` and window equal the table exactly; an
    out-of-range or unknown profile resolves to Balanced with a cause, never partially; no grant
    exceeds `quantum_bytes`; no queue exceeds `max_queued_bytes`; no grant delay exceeds the
    profile's `max_grant_delay_ms`, itself never above 250 ms; the pack stays off unless the
    operator sets both the cargo feature and `BAFT_TUNNEL_POLICY_PACK`. A profile is reported
    `measured` only when a result record for its exact (engine, carrier, profile) tuple carries an
    evidence level and the run's SHA (ADR-0047), and the scheduler arm keeps its `simulated` label.

20. **Nothing in TP-0.2 needs an owner decision.** The taxonomy above is derived from the code and
    the TP-2.2 evidence. Owner-gated items stay outside this acceptance: the two-host LAB window
    for real-path evidence, acceptance of ADR-0046 (UDP) before any Gaming/Trading
    head-of-line-blocking claim, and any tag or release.

## Known limits (recorded in Phase 0)

- A single TCP carrier can still cause head-of-line blocking; the Gaming/Trading benefit may be limited without a datagram path (QUIC/UDP is not implemented, roadmap Phase 5). See decision 11.
- Kernel socket buffers contribute to bufferbloat and may be adjustable only partially without privileges.
- Impairment tooling for benchmarks without `CAP_NET_ADMIN` on LAB hosts needs a design (CI namespaces or approved LAB method).

## Delivery order and priority

Phases 0-5 as in the plan. The pack starts only after Stable 0.2.0 and must not delay core work (roadmap queue, Phase 6 hardening). LAB use is serialized with the 0.2.0 acceptance work. If Phase 0 shows that order or dependencies must change, this ADR is amended with the technical reason.

## Consequences

- Positive: a path to measurable, per-use-case tuning without new engines or privileged access.
- Cost: a policy layer in the hot path needs an overhead budget and fairness tests (TP-2.2, closed).
- Cost of the accepted taxonomy: with one class per session, profiles differ only in window,
  quantum, cap and bounded wait, so arbitration weights stay identity until a session can mix
  classes (decision 16). A profile's benefit must therefore be shown by its parameters, not by
  prioritisation.
- Risk: benefit may be small on TCP-only carriers; the evidence gate allows dropping a profile that does not beat Balanced.

## Rejected alternatives

- Separate engines per use case (duplication, larger attack surface).
- Kernel tc/qdisc shaping (needs `CAP_NET_ADMIN`, rejected by owner in ADR-0030).
- Traffic classification by payload inspection (privacy, complexity, fragile).
