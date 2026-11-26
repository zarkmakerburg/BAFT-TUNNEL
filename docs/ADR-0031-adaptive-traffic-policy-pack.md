# ADR-0031: Adaptive Traffic Policy Pack

**Status:** Accepted (2026-11-26, TP-0.2 / #228); the schema is implemented (TP-3.1 / #233, 2026-11-26, see the implementation note). The runtime stays gated: everything is behind the `policy-pack` cargo feature and the `BAFT_TUNNEL_POLICY_PACK` runtime switch, both off by default. Acceptance fixes the design and the taxonomy; it accepts no profile as measured and claims no benefit.
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

11. **UDP/datagram carrier is a prerequisite for Gaming/Trading benefit claims, and a separate work item** ([#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244)). It needs its own ADR, independent security audit, acceptance matrix and real two-host LAB. It is not part of this pack. Order: after Stable 0.2.0; not needed for the policy engine (Phase 2), Balanced, Streaming or Bulk, which run on TCP/Yamux; it must land before any Gaming/Trading claim that depends on avoiding TCP head-of-line blocking. The profile definitions and their TCP measurements do not wait for it: those two profiles may be built and measured on TCP, and claim only tunnel-internal queueing effects until it lands.
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
    versioned set of three parameters resolved into knobs the pack already has: the per-session
    Yamux stream window (`window_bytes`, ADR-0048 decision 4), and the deficit round-robin quantum
    (`quantum_bytes`) and per-flow queue cap (`max_queued_bytes`) of `SchedulerConfig`. No profile
    introduces a new transport, engine or kernel mechanism, and none carries a per-profile wait
    limit (see the bounded-wait note below).

    | Profile | id | window_bytes | quantum_bytes | max_queued_bytes | What it changes |
    |---|---|---|---|---|---|
    | Balanced (default, fail-safe) | `balanced/1` | 262144 (256 KiB) | 16384 (16 KiB) | 65536 (64 KiB) | Identity: exactly the pack-on parameters TP-2.1/TP-2.2 measured, and the target every profile falls back to |
    | Gaming | `gaming/1` | 262144 (256 KiB) | 4096 (4 KiB) | 16384 (16 KiB) | Quarter of Balanced's quantum and cap: the in-flight ceiling stays at the carrier floor, but each grant and each buffer is smaller, so a bulk burst occupies a smaller share of the queue at any moment |
    | Trading | `trading/1` | 262144 (256 KiB) | 2048 (2 KiB) | 8192 (8 KiB) | The finest interleaving the pack can produce on this carrier: smallest quantum and cap, throughput deliberately sacrificed |
    | Streaming | `streaming/1` | 524288 (512 KiB) | 32768 (32 KiB) | 131072 (128 KiB) | Twice Balanced's window and quantum: sustained medium-bitrate delivery with a larger but still bounded standing buffer |
    | Bulk Transfer | `bulk/1` | 2097152 (2 MiB) | 65536 (64 KiB) | 262144 (256 KiB) | Eight times Balanced's window: throughput for bulk transfers, still half of today's unmanaged 4 MiB window, latency accepted |

    **The 256 KiB window is a floor, not a choice.** `tokio-yamux` 0.3.20 defines
    `INITIAL_STREAM_WINDOW` as 256 KiB and both ends assume it at stream creation, and
    `PolicyRelay::parse` refuses any `window-cap` below it. A lower window would need a carrier
    change, so Gaming and Trading keep the floor and differ from Balanced by quantum and cap only;
    their claims are therefore limited to tunnel-internal queueing and burst sharing, never to
    fewer in-flight bytes. This is recorded rather than papered over.

    All five rows satisfy the numeric bounds `PolicyRelay::parse` enforces (`window_bytes >=
    INITIAL_STREAM_WINDOW` = 256 KiB, `quantum_bytes >= 1024`, `quantum_bytes <= 256 KiB`,
    `max_queued_bytes >= quantum_bytes`, `max_queued_bytes <= 1 MiB`), so TP-3.x implements them
    through `SchedulerConfig` as it exists and adds no scheduler type.

    **These tuples are realised through the tunnel config, not the env grammar.** TP-3.1 resolves
    a profile into the two fields of the `PolicyRelay` struct (`stream_window`, `scheduler`) that
    `parse` already populates, after checking the numeric bounds above. The `BAFT_TUNNEL_POLICY_PACK`
    spelling is not extended: `window-cap` sets a window without a scheduler and `sched` sets a
    scheduler at the default window, which is exactly what reproduces the TP-2.1/TP-2.2 arms, and a
    profile above the default window with scheduler parameters has no reason to exist as an env
    value. Extending the grammar is therefore not a prerequisite for schema version `1`.

    **Bounded wait is not a profile parameter in schema version `1`.** Every profile is bounded by
    the existing mechanics: the arbiter's deficit round-robin rotation visits every registered
    flow, and the guarded wrapper clamps each grant's `delay` to the global `MAX_GRANT_DELAY` of
    250 ms (`src/policy.rs`). A per-profile tightening would need a deadline on
    `FlowHandle::acquire`'s wait for the arbiter, which is a mechanics change, so it is explicitly
    out of schema version 1 and out of TP-3.1 scope; adding it later needs its own decision and
    tests.

    Why these values: Balanced is not a new choice but the measured pack default; Gaming and
    Trading shrink only what the tunnel itself controls (chunk size, per-flow buffer), which is the
    only effect decision 4 permits; Streaming and Bulk widen in-flight bytes, and since ADR-0048
    decision 4 bounded the measured arm at 256 KiB, each wider value is a hypothesis to be measured
    per tuple, not a throughput promise.

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
    validated against the table in decision 15, resolved into that same `PolicyRelay` struct's
    `stream_window` and `scheduler` fields, with `balanced/1` as the identity
    profile and any invalid or unknown value falling back to Balanced with a logged cause
    (decision 9). TP-3.1 changes no mechanics: it selects one of the three existing parameters and
    the class label, nothing else. TP-3.2 Gaming, TP-3.3 Trading, TP-3.4
    Streaming and TP-3.5 Bulk each add one row of the same table plus their per-tuple evidence
    record; none may change the mechanics, only the parameters.

19. **Testable acceptance for this schema (used by TP-3.x).** For every profile row: the config
    round-trips unchanged; the resolved `SchedulerConfig` and window equal the table exactly; an
    out-of-range or unknown profile resolves to Balanced with a cause, never partially; a window
    below the 256 KiB carrier floor is refused the same way; no grant exceeds `quantum_bytes`; no
    queue exceeds `max_queued_bytes`; no grant's `delay` exceeds the global 250 ms
    `MAX_GRANT_DELAY`; the arbiter still visits every registered flow in one rotation; and the
    pack stays off unless the operator sets both the cargo feature and `BAFT_TUNNEL_POLICY_PACK`. A profile is reported
    `measured` only when a result record for its exact (engine, carrier, profile) tuple carries an
    evidence level and the run's SHA (ADR-0047), and the scheduler arm keeps its `simulated` label.

20. **Nothing in TP-0.2 needs an owner decision.** The taxonomy above is derived from the code and
    the TP-2.2 evidence. Owner-gated items stay outside this acceptance: the two-host LAB window
    for real-path evidence, acceptance of ADR-0046 (UDP) before any Gaming/Trading
    head-of-line-blocking claim, and any tag or release.

## Implementation note 2026-11-26 (TP-3.1 / #233): the profile schema as shipped

The schema of decision 15 is in the code, behind the same two gates. What shipped, and the
coding-level choices this ADR left open:

- **Field.** `policy` is an object on both the `serve` and `forward` tunnel configs with exactly
  one member, `policy.profile`, holding a versioned id: `balanced/1`, `gaming/1`, `trading/1`,
  `streaming/1`, `bulk/1`. Any other member inside `policy` is refused by `deny_unknown_fields`,
  like the rest of the tunnel config. `src/policy_profile.rs` owns the ids and the table;
  `config::PolicySettings` is the config type.
- **Validation, and one supersession.** The "missing or unknown means `balanced/1` ... with a
  logged cause" wording of decisions 16, 18 and 19 is split in the implementation: **absent**
  means the identity profile, and **a named but unsupported or unknown id is a hard config error**
  whose message lists the five accepted values and the schema version. Falling back on a typo
  would silently apply numbers the operator did not ask for; failing before any tunnel starts is
  the fail-closed reading of decisions 2 and 9 and is what TP-3.1's acceptance asks for. Nothing
  else in those decisions changes.
- **Engine scope enforced, not implied.** A profile on `engine: "baft"` is refused with a message
  naming the two Rust engines (decision 17) instead of being accepted and ignored.
- **Resolution.** `PolicyRelay::from_env_and_profile` (feature build) is the only place a profile
  becomes numbers: it fills the same `stream_window` and `SchedulerConfig` fields `parse` already
  fills. `BAFT_TUNNEL_POLICY_PACK` still decides whether the pack runs at all; when it is on and a
  profile is configured, the profile supplies the window, quantum and cap and the switch's numeric
  arguments are ignored (they reproduce the TP-2.1/TP-2.2 arms). With no profile the switch's
  numbers are used unchanged, so the default data path is byte-identical to the commit before
  TP-3.1 — and `balanced/1` equals `BAFT_TUNNEL_POLICY_PACK=sched`.
- **Plumbing.** `ServerOptions`/`ForwardOptions` carry `profile: Option<PolicyProfile>`; the session
  window comes from `yamux_config_for(max, profile)` on both session ends and on the protocol-v3
  resume session, and the scheduler from `policy_session(profile)` on the two Yamux relay paths —
  server and forward — which are the only paths that ever had it. The resumable-stream relay
  (`resume_stream`) keeps its own buffering and budget and is untouched: a profile's quantum and cap
  bound Yamux logical streams, not resumed substreams, exactly as `BAFT_TUNNEL_POLICY_PACK=sched`
  already did. No new module, task or config path is added.
- **Class label.** The profile id *is* the class id; no separate `Class` mapping is wired yet,
  because class weights stay identity in schema version 1 (decision 16). The mapping lands with the
  weights that need it.
- **Not in this slice.** No CLI flag or panel field for the profile (TP-5.1), no change to the env
  grammar, no per-profile wait deadline, no arbitration weights, no measurement: every profile
  stays a definition and the scheduler arm keeps its `simulated` label.
- **Tests.** `src/policy_profile.rs` (the five rows, the default, the rejection message, config
  deserialization), `src/config.rs` (field parsed, absent, unknown id, unknown member, baft
  refusal) and `src/policy_relay.rs` (each profile's exact numbers, switch-off wins, the
  no-profile path equal to the switch, the table inside the relay bounds), all run by
  `cargo test --all-targets` in both feature configurations.

## Implementation note 2026-11-26 (TP-3.2 / #234): the Gaming row and its evidence record

What shipped, and the choices this ADR left open for TP-3.2:

- **No mechanics, only the row.** `gaming/1` already resolved through `policy.profile` when TP-3.1
  shipped; TP-3.2 adds no scheduler type, no config field and no tuning knob. "Interactive
  priority, small queues, bufferbloat control" is realised on this carrier as the row's 4 KiB
  quantum and 16 KiB hard per-flow cap being applied to every flow of a single-class session — the
  smallest thing decisions 15 and 16 permit. Intra-session prioritisation would need per-class
  weights and a multi-service ingress the tunnel config cannot express (decision 16), so no
  priority weights were added and the issue title's "priority" is not claimed as queue-jumping
  between flows.
- **Why Gaming keeps Balanced's window.** 256 KiB is the carrier floor (`INITIAL_STREAM_WINDOW`),
  so Gaming differs from Balanced only in quantum and cap, exactly as decision 15 recorded. A
  profile cannot put fewer bytes in flight; it can only hold less per flow in the relay's queues.
- **Evidence record.** `docs/POLICY_PACK_TP-3.2_GAMING_EVIDENCE.md` is the per-tuple record
  decisions 19 and ADR-0047 ask for. It carries `simulated` numbers from the repo harness at this
  commit and lists what the harness cannot settle — real-path latency under loss, jitter, path
  change and carrier kill, RSS, the 60-minute run, and any head-of-line-blocking claim (needs the
  UDP carrier, #244) — as not measured. The `(tcp-tls, TCP, gaming/1)` and `(noise-tcp, TCP,
  gaming/1)` tuples therefore stay `unknown`: the harness runs the product relay and Yamux over
  loopback TCP with no engine handshake, so its lines are neither tuple. Nothing is `measured`, no
  benefit is claimed, and no profile is "ready".
- **The harness measures the shipped table.** Test-only `Mechanism::for_profile` (feature build)
  maps a `PolicyProfile` to the harness arm by reading the same accessors `policy_relay` resolves,
  so the record cannot drift from the code; `profile_arms_are_the_shipped_rows` asserts that
  `balanced/1` reproduces the TP-2.2 arm and every row maps to its exact numbers.
- **Enabling it.** Unchanged from TP-3.1, and the three inputs do different jobs: the `policy-pack`
  feature and the `BAFT_TUNNEL_POLICY_PACK` switch gate activation, and `policy.profile: "gaming/1"`
  selects the row. All three are needed to select Gaming. Without the switch (and without the
  feature) nothing activates and the path is today's. With the switch set but no profile, the pack
  still runs on that switch's own numbers (`sched`, i.e. Balanced's) — an absent profile does not
  turn the pack off and does not restore the pack-off path.
- **Status.** TP-3.2 stays open on its acceptance in the pack plan §6 (latency and jitter under
  bulk competition, loss, throughput, CPU/RSS, fairness, impairment scenarios, long run, evidence
  labels). The one remaining requirement is a real two-host LAB window for the (engine, carrier,
  profile) tuples, which is suspended by owner decision; this slice ships the implementation and
  the `simulated` record, not a measured profile.

## Implementation note 2026-11-26 (TP-3.3 / #235): the Trading row and its evidence record

What shipped, and the choices this ADR left open for TP-3.3:

- **No mechanics, only the row.** `trading/1` already resolved through `policy.profile` when TP-3.1
  shipped; TP-3.3 adds no scheduler type, no config field and no tuning knob. The row is 256 KiB
  window / 2 KiB quantum / 8 KiB cap, i.e. the finest interleaving the pack can produce on this
  carrier, and 256 KiB is the carrier floor, so Trading differs from Balanced only in quantum and
  cap — the smallest thing decisions 15 and 16 permit. Nothing was added to make it "more
  interactive"; intra-session weights stay excluded (decision 16), so the class label is still the
  only thing the configured profile changes besides those two numbers.
- **Why its numbers are not tuned further.** The ADR documents Trading as "throughput deliberately
  sacrificed". The harness shows the 2 KiB quantum does not in fact reduce the sender's handoff
  rate against `balanced/1` at 2.5 MB/s (1.6% fewer bytes handed off at the median, a sender-side
  count that is not delivered goodput) but does cost about 4.4× the CPU per delivered MB, and does
  not improve the loaded p99 (it is 1.5-21.2% above Balanced in every repeat, 10.3% at the median).
  That is the row decision 15 fixed, measured; changing quantum or cap to chase a result would be a
  new decision, not this item.
- **Evidence record.** `docs/POLICY_PACK_TP-3.3_TRADING_EVIDENCE.md` is the per-tuple record
  decisions 19 and ADR-0047 ask for. It carries `simulated` numbers from the repo harness at this
  commit and lists what the harness cannot settle — real-path latency under loss, jitter, path
  change and carrier kill, RSS, the 60-minute run, and any head-of-line-blocking claim (needs the
  UDP carrier, #244) — as not measured. Unlike TP-3.2's record it includes the idle arm, because
  criterion 1 names idle latency and the harness can supply it per arm; TP-3.2's record was left as
  shipped rather than rewritten.
- **The harness measures the shipped table.** Test-only `Mechanism::for_profile` (feature build)
  maps a `PolicyProfile` to the harness arm by reading the same accessors `policy_relay` resolves,
  and `profile_arms_are_the_shipped_rows` now also pins the `trading/1` literal
  (`sched-w262144-q2048-cap8192`) plus its difference from Gaming and Balanced. The invalid and
  absent-profile paths are the TP-3.1 tests that still guard this row:
  `every_ratified_profile_resolves_to_its_exact_numbers`, `unsupported_names_are_rejected_and_the_error_names_the_accepted_values`,
  `config_deserialization_rejects_an_unsupported_name_at_parse_time`, `no_profile_is_balanced`,
  `a_configured_profile_supplies_the_ratified_numbers_and_the_switch_only_enables_them` and
  `no_profile_keeps_the_switch_numbers_byte_for_byte`.
- **Enabling it.** Unchanged from TP-3.1 and TP-3.2: the `policy-pack` feature and the
  `BAFT_TUNNEL_POLICY_PACK` switch gate activation, and `policy.profile: "trading/1"` selects the
  row. All three are needed to select Trading. An absent profile does not turn the pack off: with
  the switch set the pack runs on the switch's own (`sched`, i.e. Balanced) numbers, and with the
  switch unset the path is today's byte for byte.
- **Status.** TP-3.3 stays open on its acceptance in the pack plan §6 (idle and bulked latency and
  jitter, loss, throughput, CPU/RSS, fairness, impairment scenarios, long run, evidence labels).
  The one remaining requirement is a real two-host LAB window for the (engine, carrier, profile)
  tuples, suspended by owner decision; this slice ships the `simulated` record and no measured
  profile. The record also puts one question to that window: whether Trading is worth keeping when
  it does not beat Balanced even in the harness and costs several times the CPU per MB.

## Implementation note 2026-11-26 (TP-3.4 / #236): the Streaming row and its evidence record

What shipped, and the choices this ADR left open for TP-3.4:

- **No mechanics, only the row.** `streaming/1` already resolved through `policy.profile` when TP-3.1
  shipped; TP-3.4 adds no scheduler type, no config field and no tuning knob. The row is 512 KiB
  window / 32 KiB quantum / 128 KiB cap — exactly when decision 15 fixed for Streaming. Nothing was
  added to make it "smoother" or to pace a stream; the only thing the configured profile changes
  besides that row is the class label, and intra-session weights stay excluded (decision 16).
- **Why its numbers are not tuned further.** The row's stated purpose is sustained medium-bitrate
  delivery through a larger but still bounded standing buffer, and the harness shows both sides of
  that trade: the sender hands off 3.8% more bytes per second than `balanced/1` and 7.3% more across
  three competing flows (sender-side counts, not delivered goodput), CPU per MB is 38.3% lower with
  no overlap between the three repeats, and loaded p50/p95/p99 are consistently about 2.2× Balanced
  because an interactive ping waits behind the larger buffer. That is the row decision 15 fixed,
  measured; it shows no delivered-throughput benefit and no real-path latency, so the profile is not
  tuned toward a result this harness cannot verify. Changing window, quantum or cap would be a new
  decision, not this item.
- **Evidence record.** `docs/POLICY_PACK_TP-3.4_STREAMING_EVIDENCE.md` is the per-tuple record
  decisions 19 and ADR-0047 ask for. It carries `simulated` numbers from the repo harness at this
  commit and lists what the harness cannot settle — real-path latency under loss, jitter, path
  change and carrier kill, loss, RSS, the 60-minute run and delivered goodput — as not measured. Like
  TP-3.3's record it includes the idle arm, because criterion 1 names idle latency.
- **The harness measures the shipped table.** Test-only `Mechanism::for_profile` (feature build)
  maps a `PolicyProfile` to the harness arm by reading the same accessors `policy_relay` resolves,
  and `profile_arms_are_the_shipped_rows` now also pins the `streaming/1` literal
  (`sched-w524288-q32768-cap131072`) plus its difference from Balanced and Trading. The invalid,
  absent and resolved paths are pinned by the new
  `streaming_profile_path_and_its_fallbacks_are_unchanged`, on top of the TP-3.1 tests that already
  cover every ratified row (`every_ratified_profile_resolves_to_its_exact_numbers`,
  `unsupported_names_are_rejected_and_the_error_names_the_accepted_values`,
  `config_deserialization_rejects_an_unsupported_name_at_parse_time`, `no_profile_is_balanced`,
  `a_configured_profile_supplies_the_ratified_numbers_and_the_switch_only_enables_them` and
  `no_profile_keeps_the_switch_numbers_byte_for_byte`).
- **Enabling it.** Unchanged from TP-3.1 to TP-3.3: the `policy-pack` feature and the
  `BAFT_TUNNEL_POLICY_PACK` switch gate activation, and `policy.profile: "streaming/1"` selects the
  row. An absent profile does not turn the pack off: with the switch set the pack runs on the
  switch's own (`sched`, i.e. Balanced) numbers, and with the switch unset the path is today's byte
  for byte.
- **Status.** TP-3.4 stays open on its acceptance in the pack plan §6 (idle and bulked latency and
  jitter, loss, throughput, CPU/RSS, fairness, impairment scenarios, long run, evidence labels).
  The one remaining requirement is a real two-host LAB window for the (engine, carrier, profile)
  tuples, suspended by owner decision; this slice ships the `simulated` record and no measured
  profile. The record puts three questions to that window: whether the 38% CPU reduction survives a
  real engine handshake and rates, whether the sender-handoff advantage becomes delivered goodput on
  a path with real RTT and loss, and whether the roughly doubled loaded p50 is acceptable for the
  profile's target use.

## Implementation note 2026-11-26 (TP-3.5 / #237): the Bulk row and its evidence record

What shipped, and the choices this ADR left open for TP-3.5:

- **No mechanics, only the row.** `bulk/1` already resolved through `policy.profile` when TP-3.1
  shipped; TP-3.5 adds no scheduler type, no config field and no tuning knob. The row is 2 MiB window
  / 64 KiB quantum / 256 KiB cap — exactly what decision 15 fixed for Bulk, and eight times Balanced's
  window. Nothing was added to make it "faster" or to widen it further: widening beyond its stated
  purpose would be a new decision, not this item. The only thing the configured profile changes
  besides the row is the class label, and intra-session weights stay excluded (decision 16).
- **Why its numbers are not tuned further.** The row's stated purpose is maximum efficiency for
  large transfers, and the harness shows both sides of that trade: the sender hands off 11.9% more
  bytes per second than `balanced/1` (sender-side counts, not delivered goodput), CPU per delivered
  MiB is 48.9% lower with no overlap between the three repeats, and loaded p50/p95/p99 are
  consistently about 9.4× Balanced because an interactive ping waits behind an eight-times-larger
  buffer (still 2.0× below the unmanaged Baseline). A smaller window would buy back interactive
  latency at the cost of the row's purpose; the split is decision 15's, now measured, and the record
  puts the acceptability of that price to the LAB window rather than inventing a latency bound here
  — this ADR states none, so none was added after seeing the number.
- **Goodput versus CPU: what this box can and cannot settle.** The item's first acceptance item is
  goodput versus Balanced and Baseline. The harness counts application-side sender handoffs, and its
  shaped bottleneck saturates every arm, so a sink-side figure on this box would measure the shaper,
  not the profile; TP-3.5 therefore measures CPU per GiB (the second acceptance item) and records
  delivered goodput as not measured instead of adding a counter that could not answer the question.
  The wider row's CPU reduction is the one measured efficiency gain, and it says nothing about
  throughput.
- **Evidence record.** `docs/POLICY_PACK_TP-3.5_BULK_EVIDENCE.md` is the per-tuple record decisions
  19 and ADR-0047 ask for. It carries `simulated` numbers from the repo harness at this commit and
  lists what the harness cannot settle — delivered goodput, real-path latency under loss, jitter,
  path change and carrier kill, loss, RSS, the 60-minute run and the two-host window — as not
  measured. Like TP-3.3's and TP-3.4's records it includes the idle arm, because criterion 1 names
  idle latency.
- **The harness measures the shipped table.** Test-only `Mechanism::for_profile` (feature build)
  maps a `PolicyProfile` to the harness arm by reading the same accessors `policy_relay` resolves,
  and `profile_arms_are_the_shipped_rows` now also pins the `bulk/1` literal
  (`sched-w2097152-q65536-cap262144`) plus its difference from Balanced and Streaming. The invalid,
  absent, switch-off and resolved paths are pinned by the new
  `bulk_profile_path_and_its_fallbacks_are_unchanged`, on top of the TP-3.1 tests that already cover
  every ratified row.
- **Enabling it.** Unchanged from TP-3.1 to TP-3.4: the `policy-pack` feature and the
  `BAFT_TUNNEL_POLICY_PACK` switch gate activation, and `policy.profile: "bulk/1"` selects the row.
  An absent profile does not turn the pack off: with the switch set the pack runs on the switch's own
  (`sched`, i.e. Balanced) numbers, and with the switch unset the path is today's byte for byte — a
  configured profile never enables the pack on its own.
- **Status.** TP-3.5 stays open on its acceptance in the pack plan §6 and in #237 (goodput and CPU per
  GiB, the impact on co-running interactive flows, fairness, long run, evidence labels). Fairness,
  CPU and harness latency are recorded; **delivered goodput, RSS, loss, impairment scenarios and the
  long run are not measured**, and the one requirement that can settle the item is a real two-host
  LAB window for the (engine, carrier, profile) tuples, suspended by owner decision. This slice ships
  the `simulated` record and no measured profile. The record puts three questions to that window:
  whether the 48.9% CPU reduction survives a real engine handshake and rates, whether the higher
  sender handoff becomes delivered goodput on a path with real RTT and loss, and whether the 9.4×
  loaded interactive p50 is an acceptable price for the widest row.

## Implementation note 2026-11-26 (TP-4.1 / #238): adaptive control settled by ADR-0056

Decision 9's bullet list is now specified rather than left to implementation. [ADR-0056](ADR-0056-adaptive-optimization-controller.md) fixes the slow loop:

- **Tunable.** The controller chooses one of the five rows of decision 15 and never computes a
  number; the window is not adaptable (Yamux assumes it at stream creation, ADR-0048 decision 4),
  so only the scheduler pair moves, one rung at a time down the ladder
  `trading/1 < gaming/1 < balanced/1 < streaming/1 < bulk/1` and never above the configured row.
- **Inputs and guards.** 1 Hz samples of the pack's own counters (a queued-bytes/grant-rate
  queueing-delay estimate, `overflow_refusals`, `Guarded::violations`), 3-sample persistence,
  50 ms down / 20 ms up hysteresis, 30 s dwell, 3 changes per 300 s, a 60 s verification with a
  10% improvement floor and a 600 s cooldown for a failed row, immediate guard/overflow fail-safe
  to the configured row, and the configured row restored after three missed samples. Every change
  is one log line naming its cause and the estimate that triggered it.
- **Off by default.** `policy-pack` and `BAFT_TUNNEL_POLICY_PACK` still gate the pack, and
  adaptation additionally needs a `policy.adaptive` opt-in (default false) that the wiring slice
  will add; decision 2's fail-safe target and this ADR's profile rows are unchanged. No profile is
  measured and no benefit is claimed by adaptation.
- **Shipped so far.** The pure decision core in `src/policy_adaptive.rs` and its unit tests, with
  no caller and no config field; the wiring, benchmark and LAB items stay open in TP-4.1.

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
