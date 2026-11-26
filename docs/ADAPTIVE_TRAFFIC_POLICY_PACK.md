# Adaptive Traffic Policy Pack: plan

**Status: Accepted design, profiles not yet accepted (ADR-0031 accepted 2026-11-26). Phase 0 is closed**, the Phase 2 interface, the queue/scheduler core, the ADR-0048 in-flight bound and the scheduler's wiring into the relay exist behind the `policy-pack` cargo feature (off by default; the bound and the scheduler are reachable only through the `BAFT_TUNNEL_POLICY_PACK` runtime switch). **No profile is accepted as measured, and every measurement is harness/`simulated` — the scheduler arm measured as neither a win nor a regression and stays off by default. TP-3.1 (#233) is closed: the tunnel config accepts an optional `policy.profile` with the five versioned ids, each resolving to its ADR-0031 row in the existing window/`SchedulerConfig` fields; absent keeps today's numbers, an unknown id is refused with the accepted values. Next executable step: TP-3.2 (#234), `gaming/1` plus its evidence record.**
Decision: [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md). Epic: [#226](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/226). Roadmap entry: [PRODUCT_ROADMAP.md §5a](PRODUCT_ROADMAP.md).

### Current implementation state (2026-11-26)

| Item | State | Where |
|---|---|---|
| TP-0.1 research (#227) | Delivered as research: hook points, TCP/UDP limits, unprivileged OS options; no measurement | [POLICY_PACK_TP-0.1_RESEARCH.md](POLICY_PACK_TP-0.1_RESEARCH.md) |
| TP-2.0 interface (#245) | Implemented: versioned interface, capability declaration that defaults to `Unknown`, fail-safe `Guarded` wrapper | `src/policy.rs` (feature `policy-pack`), unit tests |
| TP-2.1 queues/scheduler (#231) | Implemented and wired: one deficit round-robin arbiter per relay direction, bounded per-flow queues (one quantum in flight, 64 KiB cap), bounded wait; reachable with `BAFT_TUNNEL_POLICY_PACK=sched[:<quantum>[:<cap>]]` | `src/policy_sched.rs`, `src/policy_relay.rs` (feature `policy-pack`), unit tests |
| TP-0.2 ADR acceptance (#228) | Closed 2026-11-26. ADR-0031 is **Accepted** with schema version 1: five profiles with exact window/quantum/cap values (window floored at the carrier's 256 KiB minimum, so Gaming/Trading differ by quantum and cap), class = the tunnel's configured `policy.profile` (one class per session, so arbitration weights stay identity), engine scope = `tcp-tls` + `noise-tcp` on TCP/Yamux (`baft` and the UDP carrier excluded) | [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decisions 15-20 |
| TP-1.1/1.2 measurement (#229/#230) | Harness only (`simulated`): Baseline scenarios, window-cap experiment, ADR-0048 mechanism matrix (M1/M2/M3, the window arm and the scheduler arm, three rates), aged-session scenario | `src/policy_baseline.rs`, [POLICY_PACK_TP-0.1_RESEARCH.md](POLICY_PACK_TP-0.1_RESEARCH.md) §9.2-9.4 |
| TP-2.1 in-flight mechanism (M1/M2/M3) | Decided 2026-11-26 by measurement: the Yamux per-stream window (256 KiB); M1 works only at 2 MiB and cannot go lower, M2/M3 measured equal to Baseline | [ADR-0048](ADR-0048-policy-queues-and-scheduler.md) decision 4 and results |
| Relay wiring (#244) | Both levers: `tunnel::yamux_config` applies the pack's window for `window-cap[:<bytes>]`, and `tunnel::relay_stream` runs the scheduled relay for `sched[:<quantum>[:<cap>]]`; an unset or unknown switch keeps `relay_with_idle_timeout`, i.e. today's bytes exactly | `src/policy_relay.rs`, `src/tunnel.rs` (feature `policy-pack`), unit tests |
| TP-2.2 fairness/overhead (#232) | Closed 2026-11-26 (`simulated`). Single flow: level with the window arm at 1 / 2.5 / 5 MB/s. Multi-flow (2 and 3 flows, plus a mid-run close): delivery split 1.000 in every arm, per-flow p50/p95 within 1% of the window pump, survivors keep moving. CPU: +1.4% / +4.3% / −8.2% per delivered MB against Baseline and +0.7% on the unshaped per-MB probe, idle p50 within 73 µs — decision 6's budget met with the margin stated, not certified below the harness's per-run spread. **No reproducible benefit, so the switch stays off by default.** Classes are not measured (every flow is `Balanced`; TP-3.x). Unit tests cover arbitration, the cap, closed flows, multi-flow fairness and churn | [ADR-0048](ADR-0048-policy-queues-and-scheduler.md), [POLICY_PACK_TP-0.1_RESEARCH.md](POLICY_PACK_TP-0.1_RESEARCH.md) §9.4-9.6 |
| TP-3.1 profile schema and Balanced (#233) | Closed 2026-11-26. One versioned `policy.profile` in the tunnel config, validated at parse and resolved into `SchedulerConfig` + the session window: `balanced/1` identity (equals `BAFT_TUNNEL_POLICY_PACK=sched`), absent keeps today's path, unknown ids refused with the accepted values listed, `baft` refused. Still behind both gates; no measurement, no benefit | [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) implementation note, `src/policy_profile.rs` |
| TP-3.2-TP-3.5, TP-4.x, TP-5.x, TP-U (#234-#244) | Proposed. **TP-3.2 (#234), Gaming: `gaming/1` plus its per-tuple evidence record, is the next executable step**; each later profile adds one row of the same table, then adaptive control, API/CLI/panel/docs and the UDP carrier follow | issues #234-#244 |

With the pack off the data path is exactly today's; nothing in this table is a benefit claim, and every number from the harness keeps the `simulated` label.

## 1. Scope

An optional add-on that tunes how BAFT-TUNNEL carries traffic, using one shared engine and five profiles: **Balanced** (default and fail-safe), **Gaming**, **Trading**, **Streaming**, **Bulk Transfer**. It adds a Traffic Policy Engine (queues, priorities, scheduling, fairness, starvation prevention) and Adaptive Optimization (slow, bounded, reversible tuning from measured conditions).

Out of scope: user/UUID/subscription/quota management, VPN protocols, replacing Xray or its panels, kernel shaping, `CAP_NET_ADMIN`, claims about internet path latency or distance.

## 2. Priority and position

Strictly after Stable 0.2.0 and the Phase 6 hardening gates. It is not part of the MVP and must not delay core work. Phase 0 (research, ADR) is documentation only and may run alongside; Phases 1-5 start after 0.2.0 ships. LAB is shared: one user at a time, only after the 0.2.0 acceptance thread releases it.

## 3. Phases, work items, status

Item status lives in the table above (§ Current implementation state); the phase list below keeps the fixed order. TP-0.1, TP-0.2, TP-2.0, TP-2.1, TP-2.2 and TP-3.1 are closed; TP-3.2 is the next executable item; the rest are Proposed.

| Phase | Item | Issue | Needs |
|---|---|---|---|
| 0 Research and architecture | TP-0.1 research: hook points, TCP/UDP, OS limits | [#227](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/227) | research |
| 0 | TP-0.2 accept ADR-0031, schema, engine scope | [#228](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/228) | ADR |
| 1 Measurement | TP-1.1 in-tunnel metrics | [#229](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/229) | ADR |
| 1 | TP-1.2 benchmark harness and Baseline | [#230](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/230) | benchmark, LAB later |
| 2 Policy engine | TP-2.0 engine-agnostic policy interface, per-engine capabilities | [#245](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/245) | ADR |
| 2 | TP-2.1 queues, scheduler, fairness | [#231](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/231) | ADR, benchmark |
| 2 | TP-2.2 starvation, fairness, overhead tests | [#232](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/232) | benchmark |
| 3 Static profiles | TP-3.1 schema and Balanced | [#233](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/233) | ADR |
| 3 | TP-3.2 Gaming (next) | [#234](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/234) | benchmark, LAB |
| 3 | TP-3.3 Trading | [#235](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/235) | benchmark, LAB |
| 3 | TP-3.4 Streaming | [#236](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/236) | benchmark, LAB |
| 3 | TP-3.5 Bulk Transfer | [#237](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/237) | benchmark, LAB |
| 4 Adaptive | TP-4.1 controller, hysteresis | [#238](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/238) | ADR, benchmark, LAB |
| 4 | TP-4.2 fail-safe, rollback, multi-condition tests | [#239](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/239) | benchmark, LAB |
| 5 Integration and UX | TP-5.1 API and CLI | [#240](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/240) | ADR |
| 5 | TP-5.2 panel (fa/en) | [#241](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/241) | UI QA |
| 5 | TP-5.3 documentation | [#242](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/242) | results |
| Prerequisite track | TP-U UDP/datagram carrier (separate transport feature; needed for Gaming/Trading claims that depend on avoiding TCP head-of-line blocking) | [#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244) | ADR, security audit, benchmark, LAB |

### UDP, other engines and per-engine rules (amendment 2026-11-15)

- **UDP/datagram carrier (#244):** its own ADR, independent audit, acceptance matrix and real two-host LAB. After Stable 0.2.0. Not needed for Phase 2, Balanced, Streaming or Bulk. It must land before any Gaming/Trading claim that depends on avoiding TCP head-of-line blocking; until then those profiles make only tunnel-internal queueing claims on TCP.
- **Engine-agnostic interface (#245):** engines declare capabilities (priority enforcement, write pacing, RTT/queue observation, datagram). Missing capability means Unsupported/Unknown.
- **What policy can control:** admission, queueing, scheduling and pacing before data reaches the engine, plus backpressure. **What it cannot:** internal queues, congestion window, retransmission, FEC and wire behaviour of engines with their own congestion control (BAFT engine, ADR-0019 separate binary; a future QUIC engine). Effect there is measured and may be nil. BAFT engine stays out of scope until a later ADR.
- **Per-engine acceptance:** a claim belongs to (engine, carrier, profile). No engine inherits another's results; each tuple needs its own benchmarks vs Balanced and Baseline with an evidence level; otherwise it shows Unsupported/Unknown.

The suggested order is unchanged from the request (0 to 5), with TP-2.0 first in Phase 2 and the UDP track running separately. One dependency note: the measurement foundation (Phase 1) must precede the engine because the overhead budget and every acceptance check rely on it. Phase 5 API/CLI (TP-5.1) can start with TP-3.1 since the schema exists then.

## 4. Dependencies and prerequisites

- Phase 0 needs only the current source and ADR-0018/0024/0030. Phases 1-5 need Stable 0.2.0 shipped.
- Phase 1 reuses ADR-0024 style metrics (unit, interval, provenance, freshness).
- Phases 3-4 acceptance needs a repeatable impairment method that works without `CAP_NET_ADMIN` on LAB hosts (open question, TP-1.2).
- Panel work reuses the existing agent API and fa/en panel conventions.

## 5. Risks and decisions required

| Risk | Handling |
|---|---|
| TCP carrier head-of-line blocking limits Gaming/Trading gains; no UDP path yet | TP-0.1 quantifies it; evidence gate may conclude "no gain over Balanced" and the profile is dropped or documented as limited |
| Policy layer adds CPU/latency in the hot path | overhead budget met as measured in TP-2.2 (+1.4%/+4.3%/−8.2% per delivered MB, idle p50 within 73 µs), off by default |
| Prioritisation harms other flows | mandatory bounded-wait and share limits; fairness tests |
| Adaptive loop oscillates | hysteresis, dwell time, rate limit, fail-safe to Balanced (TP-4.x) |
| Benchmarks mislead (simulation passed off as real) | evidence labels per result; simulation never counts as LAB |
| BAFT Go engine not covered | stated limit; separate decision if wanted |
| Scope creep into user management | ADR-0031 rule 4 and out-of-scope list |

Decisions needed before code: classification method, engine coverage, schema, overhead budget, impairment method. TP-0.2 settled classification, engine coverage and the schema (ADR-0031 decisions 15-20) and TP-2.2 measured the overhead budget; the impairment method stays open with TP-1.2 because the two-host LAB is suspended.

## 6. Acceptance criteria (every profile, all required)

Each profile is compared against **Balanced** and **Baseline** (pack off), with the evidence level recorded:

1. Latency (p50/p95/p99) and jitter under idle and under competing bulk load.
2. Packet loss (measured) or loss proxy, labelled which.
3. Throughput/goodput.
4. CPU and memory overhead.
5. Fairness and no starvation of other flows (bounded wait, share bounds).
6. Congestion and network-impairment scenarios (loss, jitter, path change, carrier kill).
7. Long-run stability (maximum 60 minutes per project policy; longer is not planned).
8. Results published with evidence label: CI-simulated, VM, real two-host LAB. Simulation is never reported as LAB.

A profile with a name and settings but without this evidence is **not** ready. Production is never claimed.

## 7. Benchmark plan

1. TP-1.2 defines scenarios: idle, interactive plus bulk mix, sustained congestion, loss, jitter, carrier kill/recovery, many small flows.
2. Baseline = pack off; reference = Balanced. Each scenario repeated with fixed seeds/durations; report medians and spread.
3. Stage A, CI/VM (simulated, labelled). Stage B, real two-host LAB after 0.2.0 and by schedule, one run at a time, isolated and cleaned up.
4. Results stored in `docs/` with run IDs and exact SHAs, following the HQ evidence format.

## 8. Estimated order and delivery points

Order: TP-0.1, TP-0.2 (docs, after 0.2.0 may start earlier as docs only) -> TP-1.1, TP-1.2 -> TP-2.1, TP-2.2 -> TP-3.1 then 3.2-3.5 -> TP-4.1, TP-4.2 -> TP-5.1-5.3.

Delivery points: D0 ADR accepted; D1 measurement and baseline published; D2 policy engine behind a switch with fairness proof; D3 Balanced plus first profile with LAB evidence; D4 remaining profiles; D5 adaptive with fail-safe; D6 API/CLI/panel/docs, shipped as Preview. No dates are promised; none of these is started.
