# TP-3.5 Bulk profile: per-tuple evidence record

**Status: harness evidence only (`simulated`); the profile is not measured and not accepted.**
Item: TP-3.5 (#237), `bulk/1`. Decision: [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md)
decisions 15-20 and its TP-3.5 implementation note. Acceptance list:
[ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) §6, plus the item's own wording in
#237 (goodput and CPU per GiB versus Balanced and Baseline, the impact on co-running interactive
flows, fairness, long run). Status contract: [ADR-0047](ADR-0047-policy-status-contract.md).

This record exists to keep the claim honest, not to close the item. The row already resolved through
the TP-3.1 `policy.profile` path before this item; what TP-3.5 adds is this record, the tests that pin
the row against the shipped table, and an explicit statement of what the repository harness can and
cannot measure. **No benefit is claimed and no profile is `measured`.**

## 1. What was run

- Harness: `src/policy_baseline.rs`, test `policy_baseline::tests::bulk_profile_evidence`
  (`cargo test --release --features policy-pack -- --ignored --nocapture bulk_profile_evidence`).
- Arms, generated from the shipped table by test-only `Mechanism::for_profile` (not hand-copied
  numbers; `profile_arms_are_the_shipped_rows` asserts the mapping, now including the `bulk/1`
  literal):
  - `baseline` — the pack off, today's relay pump and today's 4 MiB Yamux window.
  - `balanced/1` — `sched-w262144-q16384-cap65536`.
  - `bulk/1` — `sched-w2097152-q65536-cap262144` (2 MiB window, 64 KiB quantum, 256 KiB cap; four
    times Balanced's window).
- Path: the product's `yamux_config` and the shipped relay (`relay_with_idle_timeout` for Baseline,
  `relay_with_policy` for the two profiles), both ends in one process, joined by a userspace
  rate-limiting proxy over loopback TCP. No TLS or Noise handshake, no NIC, no second host.
- Scenarios: `bulk_idle` (8 s, no bulk), `bulk_bulk_competing` (8 s per arm run, 3 repeats per arm),
  `bulk_multi_flow` (2 and 3 bulk flows, first closed mid-run), ping interval 20 ms, and
  `relay_cpu_probe` at 128 MiB per repeat. The idle arm follows TP-3.3's and TP-3.4's records because
  §6 criterion 1 names idle latency.
- Bottleneck rate 2,500,000 B/s (about 2.5 MB/s, the TP-2.2 rate).
- Environment: four-core Linux `4.19.0-gvisor` sandbox (`x86_64`), one process, no other load,
  2026-11-26.
- Run identity: run `tp-3.5-local-2026-11-26`, one local invocation on 2026-11-26 (166.21 s),
  `POLICY_REPEATS=3`,
  **harness revision `c0a1c54382e92c06faf612fca6f0213c614bbf44`** (the commit that added the evidence
  test; no other uncommitted source change was present). Its `git diff
  889e11f727f89340bf4a655bdf63ce2ec2030748 -- src` touches only `#[cfg(test)] mod policy_baseline`
  (the ignored evidence test and the pinning assertions), so the **product code under test is
  identical to `main` at `889e11f727f89340bf4a655bdf63ce2ec2030748`**. The console lines that
  invocation printed are reproduced unedited in Appendix A.
- Every line below is labelled `simulated`. It is not `vm` and it is not `lab`.

## 2. Idle interactive RTT (`bulk_idle`)

Microseconds; a 64-byte round trip on a session with no bulk stream. One run per arm (the scenario
collects about 300 samples in 8 s, so a repeat adds precision but not a new condition):

| Arm | samples | p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| baseline | 301 | 4,558 | 6,922 | 7,812 | 57,846 |
| `balanced/1` | 303 | 4,478 | 6,048 | 8,131 | 47,652 |
| `bulk/1` | 303 | 4,607 | 6,779 | 9,938 | 11,827 |

Reading it honestly:

- At the median the three arms are within 129 µs (`bulk/1` is +2.9% over Balanced), i.e. a 2 MiB
  window and a 64 KiB quantum add no measurable idle latency: with one flow alone the scheduler has
  nothing to arbitrate and the window is not being consumed.
- The tail figures are single-run differences of a few samples (`bulk/1` p99 9.9 ms against Balanced's
  8.1 ms; its max 11.8 ms against 47.7 ms, i.e. it looks *better* on the max here). None of that is
  repeated, so no jitter advantage or penalty is claimed in either direction.

## 3. Interactive RTT under bulk competition (`bulk_bulk_competing`)

Microseconds; the ping shares the tunnel with one saturating bulk stream. One row per repeat;
medians across the three repeats are the row below each block.

| Arm | repeat | samples | p50 | p95 | p99 | max |
|---|---|---|---|---|---|---|
| baseline | 1 | 4 | 2,106,067 | 2,145,568 | 2,145,568 | 2,145,568 |
| baseline | 2 | 4 | 2,107,982 | 2,126,098 | 2,126,098 | 2,126,098 |
| baseline | 3 | 4 | 2,115,005 | 2,131,584 | 2,131,584 | 2,131,584 |
| **baseline (median)** | | | **2,107,982** | **2,131,584** | **2,131,584** | |
| `balanced/1` | 1 | 60 | 112,353 | 119,421 | 128,385 | 128,385 |
| `balanced/1` | 2 | 60 | 112,439 | 121,061 | 127,803 | 127,803 |
| `balanced/1` | 3 | 61 | 111,472 | 118,142 | 122,297 | 122,297 |
| **`balanced/1` (median)** | | | **112,353** | **119,421** | **127,803** | |
| `bulk/1` | 1 | 8 | 1,068,037 | 1,073,298 | 1,073,298 | 1,073,298 |
| `bulk/1` | 2 | 8 | 1,051,201 | 1,061,740 | 1,061,740 | 1,061,740 |
| `bulk/1` | 3 | 8 | 1,042,858 | 1,066,593 | 1,066,593 | 1,066,593 |
| **`bulk/1` (median)** | | | **1,051,201** | **1,066,593** | **1,066,593** | |

Reading it honestly:

- Both profile arms are far below Baseline on every percentile (p50 2.11 s → 0.11-1.05 s). That is
  the in-flight bound, reproduced in this harness, not on a real path.
- `bulk/1` costs the interactive flow a great deal relative to Balanced, consistently: **9.36× at the
  p50 medians** (+835.6% relative increase), 8.93× at p95 and 8.35× at p99, and the per-repeat p50
  gap is +850.6%, +834.9% and +835.6% — never below nine times. The row holds eight times Balanced's
  window (2 MiB against 256 KiB) and four times its cap, so a competing ping waits behind a much
  larger standing buffer. That is the design cost of the widest row, measured; no latency benefit is claimed.
- `bulk/1` still keeps loaded p50 2.0× below Baseline. Its 8 samples per run are coarse: the p50 is
  usable because the gap is an order of magnitude, but the p95/p99/max columns coincide, so the tail
  shape of this arm is not resolved by this harness.
- "Jitter" here is the within-run p95−p50 / p99−p50 spread of the same samples. It is not a separate
  jitter measurement, and no propagation delay, loss or reordering is modelled.
- **The item asks that the impact on co-running interactive flows "stay within a bound". Nothing in
  ADR-0031, the pack plan or the roadmap defines a numeric bound**, and this record does not invent
  one after seeing the number. The measured impact is recorded here as a question for the LAB window
  and, if a bound is wanted, for an ADR amendment — not as a pass.

## 4. Sender handoffs in the same runs (`bulk_bulk_competing`)

Bytes the bulk sender handed to its application-side stream per requested second, same three
repeats. This is **not** delivered goodput: the harness counts writes into the application-side
duplex, starting from the bulk task's spawn (before the 500 ms pre-measurement sleep) and dividing by
the requested 8 s, so buffered data and the counted window both affect the figure. There is no
sink-side counter over an exact elapsed interval in this scenario, so #237's first acceptance item —
**goodput versus Balanced and Baseline — is not measured here**, and no throughput conclusion is drawn.

| Arm | repeat 1 | repeat 2 | repeat 3 | median |
|---|---|---|---|---|
| baseline | 2,631,680 | 2,631,680 | 2,631,680 | 2,631,680 |
| `balanced/1` | 2,123,776 | 2,123,776 | 2,156,544 | 2,123,776 |
| `bulk/1` | 2,375,680 | 2,375,680 | 2,375,680 | 2,375,680 |

`bulk/1` handed off 11.9% more bytes than `balanced/1` at the median and 9.7% fewer than Baseline.
The Baseline row's 2,631,680 B/s already exceeds the 2.5 MB/s shaped rate, which confirms these are
sender handoffs rather than bytes that crossed the bottleneck. A higher handoff count at a wider
window is consistent with the row's stated purpose (throughput for large transfers), but it is not a
delivered-goodput or bulk-share measurement and is not written up as one.

## 5. Fairness and starvation (`bulk_multi_flow`)

Three bulk flows share the bottleneck and the first is closed mid-run; the two survivors must keep
handing off bytes and share evenly (`delivery_min_over_max` compares the surviving flows' handoff
counters only, so the two-flow variant's 1.000 is trivial and is not used as evidence).

| Arm | flow bytes (closed, survivor, survivor) | echo p50 (µs), three flows | echo p95 (µs), three flows | split | continued after churn |
|---|---|---|---|---|---|
| baseline | 6,373,376 / 12,664,832 / 12,664,832 | 5,380,691 / 4,633,230 / 3,773,843 | 5,459,212 / 5,380,690 / 5,380,690 | 1.000 | yes |
| `balanced/1` | 2,965,504 / 6,897,664 / 6,897,664 | 249,068 / 253,600 / 253,598 | 383,684 / 386,047 / 386,045 | 1.000 | yes |
| `bulk/1` | 5,373,952 / 9,568,256 / 9,568,256 | 2,260,764 / 2,103,374 / 2,303,087 | 3,097,666 / 3,112,036 / 3,106,048 | 1.000 | yes |

The bulk columns are indexed: position 0 is the flow closed at the churn point, positions 1 and 2 are
the survivors, and `delivery_min_over_max` and `continued_after_churn` use those indices; the counts
are application-side handoffs, not sink-side delivery. The echo columns are **not** indexed — the
harness collects each flow's ping task in completion order and prints no flow id — so those three
figures are the three echo streams' percentiles and cannot be attributed to a named survivor; every
one of the three kept completing, and they are not used to compare arms.

All arms kept both survivors moving, handed off the closed flow's bytes up to the close, split the
survivors evenly and reported no stall. `bulk/1` handed off 46.2% more total bytes than `balanced/1`
in the same 8 s (24,510,464 against 16,760,832 across the three flows, sender side) and 22.7% fewer
than Baseline; its echoes under three-flow competition are about 9× Balanced's p50 — the same buffer
cost as section 3. No starvation and no fairness advantage over Balanced is shown. The two-flow
variant (not tabulated; split trivial) handed off 6,422,528 and 12,713,984 bytes for `bulk/1` against
4,145,152 and 11,878,400 for `balanced/1` (19.4% more in total), same shape, no stall.

## 6. CPU (`relay_cpu_probe`, unshaped loopback, 128 MiB per repeat)

Whole-process user+system CPU per delivered MiB, an upper bound on the relay's own cost because the
harness, both tunnel ends and the test app share the process. **CPU per GiB is the same figure
multiplied by 1024** (arithmetic on these values, not a separate run), which is the unit #237 asks for:

| Arm | repeat 1 | repeat 2 | repeat 3 | median µs/MiB | derived s/GiB |
|---|---|---|---|---|---|
| baseline | 3,359.4 | 3,437.5 | 3,437.5 | 3,437.5 | 3.52 |
| `balanced/1` | 3,281.2 | 4,062.5 | 3,671.9 | 3,671.9 | 3.76 |
| `bulk/1` | 1,875.0 | 1,875.0 | 1,718.8 | 1,875.0 | 1.92 |

`bulk/1` costs **48.9% less CPU per delivered MiB than `balanced/1`** and 45.5% less than Baseline at
the medians, and the ordering holds across repeats: every `bulk/1` repeat (1,718.8-1,875.0) is below
every `balanced/1` repeat (3,281.2-4,062.5) and every Baseline repeat (3,359.4-3,437.5). That is what
fewer wake-ups per MiB at a 64 KiB quantum and a 2 MiB window would predict, and it is a measured cost
*reduction* per delivered MiB — **not a throughput result**: the probe is unshaped, so all arms
deliver all 128 MiB and only the CPU differs. The absolute windows are short (0.22-0.52 s of CPU for
the 128 MiB probe, i.e. `cpu_us` 220,000-520,000 µs), and this run's Baseline median (3,437.5) is 4.4%
below TP-3.4's (3,593.8) while Balanced's (3,671.9) is 21.7% below TP-3.4's (4,687.5) — the harness's
between-run spread is large, so compare within this invocation only; the between-arm ordering is the
usable part.

## 7. Acceptance criteria, one by one

Against the list in [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) §6 and the
wording of #237, for `bulk/1` versus Balanced and Baseline:

| # | Criterion | State here | Label |
|---|---|---|---|
| 1 | Latency p50/p95/p99 and jitter, idle and under bulk load | Idle RTT (section 2) and bulk-competing percentiles (section 3) measured in the harness; **real-path latency not measured**; the loaded p95/p99 tail is unresolved because the arm collected 8 samples | `simulated` (partial) |
| 2 | Packet loss, or a loss proxy labelled as such | Not modelled: the shaper applies serialization rate only, no loss | **not measured** |
| 3 | Throughput / goodput | Sender handoff rate measured (section 4); **delivered goodput not measured** — #237's first acceptance item is therefore open | `simulated` (partial) |
| 4 | CPU and memory overhead | CPU per delivered MiB and the derived CPU per GiB measured (section 6); **RSS not measured** — one process hosts every arm, so per-arm RSS is not separable | `simulated` (CPU only) |
| 5 | Fairness, no starvation | Three-flow share with a mid-run close (section 5) | `simulated` |
| 6 | Congestion / impairment (loss, jitter, path change, carrier kill) | Not modelled | **not measured** |
| 7 | Long run (60 minutes per policy) | Longest run 8 s per arm, 166 s for the whole test | **not measured** |
| 8 | Results published with an evidence label | This file; every number above is `simulated` | done for this slice |
| — | Impact on co-running interactive flows "within a bound" (#237) | Measured at 9.36× Balanced loaded p50 (section 3); **no numeric bound exists in ADR-0031, the pack plan or the roadmap**, so this is neither a pass nor a fail and is put to the LAB/ADR | `simulated` (number), bound **undefined** |

## 8. Per-tuple claim (ADR-0047)

The claim belongs to `(engine, carrier, profile)`, and no engine inherits another's.

| Engine | Carrier | Profile | Claim | Why |
|---|---|---|---|---|
| `tcp-tls` | TCP/Yamux | `bulk/1` | **`unknown`** | No run of this tuple. The harness runs the relay and Yamux over loopback TCP with no TLS handshake; its numbers are not this tuple's. Needs the two-host LAB window |
| `noise-tcp` | TCP/Yamux | `bulk/1` | **`unknown`** | Same: no engine handshake, no real path |
| `baft` | TCP | `bulk/1` | **`unsupported`** | Out of scope by ADR-0031 decision 17 |
| any | UDP/datagram | `bulk/1` | **`unsupported`** (no carrier) | The datagram carrier is #244, not implemented. Bulk is not listed as needing it (ADR-0031 decision 11), but no datagram tuple exists |

The `simulated` harness numbers in sections 2-6 are a reference point for the future tuple runs, not
a claim for any of them. Sections 4 and 5 are sender-side counts and are not usable as
delivered-throughput claims even at that level.

## 9. What is still missing, precisely

The item's acceptance needs a real two-host LAB run — the suspended `baft-tunnel-lab` window —
covering, per `(engine, carrier, profile)` tuple: delivered goodput (the item's first acceptance item,
and the one this harness cannot supply), idle and bulked p50/p95/p99 latency and jitter with enough
samples to resolve the tail, a measured loss figure (or an explicit loss proxy), CPU per GiB and RSS,
fairness under competing bulk, impairment scenarios (loss, jitter, path change, carrier kill), and a
run up to the 60-minute project cap. Until that window is authorized, TP-3.5 stays open, no tuple
becomes `measured`, and nothing here may be quoted as a Bulk benefit. Questions this harness puts to
that window, because it cannot answer them: whether the 48.9% lower CPU per delivered MiB survives a
real engine handshake and real rates; whether the 11.9% higher sender handoff becomes delivered
goodput on a path with real RTT and loss; whether the 9.36× loaded interactive p50 is an acceptable
price for the widest row, and if not, what the bound should be — the documents currently state none,
so that decision belongs to the LAB result and an ADR amendment rather than to this record.

## 10. Reproduce

```
# at this record's commit (or any later commit that keeps the test)
POLICY_REPEATS=3 cargo test --release --features policy-pack -- \
  --ignored --nocapture bulk_profile_evidence
```

Each `POLICY_EXPERIMENT …` line is one arm run, JSON after the first object, all labelled
`simulated`. `POLICY_RATE`, `POLICY_SECONDS` and `POLICY_BYTES` change the rate, the per-arm
duration and the CPU probe size; the defaults are the figures above. Use a fresh target directory
in this sandbox (`/tmp` is 9p and already tight); the run takes about three minutes in release.

## Appendix A. Raw run output, unedited

The 27 `POLICY_EXPERIMENT` lines run `tp-3.5-local-2026-11-26` printed (3 idle, 9 bulk competing,
6 multi-flow, 9 CPU probe), followed by the test summary. `Mechanism::label()` prints the resolved
parameters, so `sched-w262144-q16384-cap65536` is `balanced/1`'s arm and
`sched-w2097152-q65536-cap262144` is `bulk/1`'s; `baseline` is the pack off.

```
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1} {"scenario":"bulk_idle","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":301,"rtt_us":{"p50":4558,"p95":6922,"p99":7812,"max":57846},"bulk_bytes":0,"cpu_us":710000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1} {"scenario":"bulk_idle","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":303,"rtt_us":{"p50":4478,"p95":6048,"p99":8131,"max":47652},"bulk_bytes":0,"cpu_us":550000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1} {"scenario":"bulk_idle","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","rate_bytes_per_s":2500000,"samples":303,"rtt_us":{"p50":4607,"p95":6779,"p99":9938,"max":11827},"bulk_bytes":0,"cpu_us":590000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"bulk_bytes_per_s":2631680} {"scenario":"bulk_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":4,"rtt_us":{"p50":2106067,"p95":2145568,"p99":2145568,"max":2145568},"bulk_bytes":21053440,"cpu_us":1090000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":2,"bulk_bytes_per_s":2631680} {"scenario":"bulk_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":4,"rtt_us":{"p50":2107982,"p95":2126098,"p99":2126098,"max":2126098},"bulk_bytes":21053440,"cpu_us":1080000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":3,"bulk_bytes_per_s":2631680} {"scenario":"bulk_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":4,"rtt_us":{"p50":2115005,"p95":2131584,"p99":2131584,"max":2131584},"bulk_bytes":21053440,"cpu_us":1150000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"bulk_bytes_per_s":2123776} {"scenario":"bulk_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":112353,"p95":119421,"p99":128385,"max":128385},"bulk_bytes":16990208,"cpu_us":1330000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":2,"bulk_bytes_per_s":2123776} {"scenario":"bulk_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":112439,"p95":121061,"p99":127803,"max":127803},"bulk_bytes":16990208,"cpu_us":1250000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":3,"bulk_bytes_per_s":2156544} {"scenario":"bulk_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":61,"rtt_us":{"p50":111472,"p95":118142,"p99":122297,"max":122297},"bulk_bytes":17252352,"cpu_us":1050000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"bulk_bytes_per_s":2375680} {"scenario":"bulk_bulk_competing","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","rate_bytes_per_s":2500000,"samples":8,"rtt_us":{"p50":1068037,"p95":1073298,"p99":1073298,"max":1073298},"bulk_bytes":19005440,"cpu_us":1170000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":2,"bulk_bytes_per_s":2375680} {"scenario":"bulk_bulk_competing","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","rate_bytes_per_s":2500000,"samples":8,"rtt_us":{"p50":1051201,"p95":1061740,"p99":1061740,"max":1061740},"bulk_bytes":19005440,"cpu_us":1180000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":3,"bulk_bytes_per_s":2375680} {"scenario":"bulk_bulk_competing","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","rate_bytes_per_s":2500000,"samples":8,"rtt_us":{"p50":1042858,"p95":1066593,"p99":1066593,"max":1066593},"bulk_bytes":19005440,"cpu_us":1260000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"flows":2} {"scenario":"bulk_multi_flow","mechanism":"baseline","evidence":"simulated","flows":2,"bulk_bytes":[6373376,16859136],"total_bulk_bytes":23232512,"delivery_min_over_max":1.000,"flow_p50_us":[2182382,2182381],"flow_p95_us":[3453645,3292469],"cpu_us":1360000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"flows":2} {"scenario":"bulk_multi_flow","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","flows":2,"bulk_bytes":[4145152,11878400],"total_bulk_bytes":16023552,"delivery_min_over_max":1.000,"flow_p50_us":[116637,115785],"flow_p95_us":[253972,253931],"cpu_us":1300000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"flows":2} {"scenario":"bulk_multi_flow","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","flows":2,"bulk_bytes":[6422528,12713984],"total_bulk_bytes":19136512,"delivery_min_over_max":1.000,"flow_p50_us":[1663534,1115415],"flow_p95_us":[2097199,2123302],"cpu_us":1280000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"flows":3} {"scenario":"bulk_multi_flow","mechanism":"baseline","evidence":"simulated","flows":3,"bulk_bytes":[6373376,12664832,12664832],"total_bulk_bytes":31703040,"delivery_min_over_max":1.000,"flow_p50_us":[5380691,4633230,3773843],"flow_p95_us":[5459212,5380690,5380690],"cpu_us":1700000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"flows":3} {"scenario":"bulk_multi_flow","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","flows":3,"bulk_bytes":[2965504,6897664,6897664],"total_bulk_bytes":16760832,"delivery_min_over_max":1.000,"flow_p50_us":[249068,253600,253598],"flow_p95_us":[383684,386047,386045],"cpu_us":1130000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","repeat":1,"flows":3} {"scenario":"bulk_multi_flow","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","flows":3,"bulk_bytes":[5373952,9568256,9568256],"total_bulk_bytes":24510464,"delivery_min_over_max":1.000,"flow_p50_us":[2260764,2103374,2303087],"flow_p95_us":[3097666,3112036,3106048],"cpu_us":1260000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":430000,"cpu_us_per_mb":3359.4}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":440000,"cpu_us_per_mb":3437.5}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":440000,"cpu_us_per_mb":3437.5}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":420000,"cpu_us_per_mb":3281.2}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":520000,"cpu_us_per_mb":4062.5}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":470000,"cpu_us_per_mb":3671.9}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":240000,"cpu_us_per_mb":1875.0}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":240000,"cpu_us_per_mb":1875.0}
POLICY_EXPERIMENT {"profile_run":"tp-3.5","scenario":"relay_cpu_probe","mechanism":"sched-w2097152-q65536-cap262144","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":220000,"cpu_us_per_mb":1718.8}
```

Test summary: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 485 filtered out;
finished in 166.21s`.
