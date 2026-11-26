# TP-3.3 Trading profile: per-tuple evidence record

**Status: harness evidence only (`simulated`); the profile is not measured and not accepted.**
Item: TP-3.3 (#235), `trading/1`. Decision: [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md)
decisions 15-20 and its TP-3.3 implementation note. Acceptance list:
[ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) §6. Status contract:
[ADR-0047](ADR-0047-policy-status-contract.md).

This record exists to keep the claim honest, not to close the item. The row itself already resolved
through the TP-3.1 `policy.profile` path before this item; what TP-3.3 adds is this record, the tests
that pin the row against the shipped table, and an explicit statement of what the repository harness
can and cannot measure. **No benefit is claimed and no profile is `measured`.**

## 1. What was run

- Harness: `src/policy_baseline.rs`, test `policy_baseline::tests::trading_profile_evidence`
  (`cargo test --release --features policy-pack -- --ignored --nocapture trading_profile_evidence`).
- Arms, generated from the shipped table by test-only `Mechanism::for_profile` (not hand-copied
  numbers; `profile_arms_are_the_shipped_rows` asserts the mapping, now including the `trading/1`
  literal):
  - `baseline` — the pack off, today's relay pump and today's 4 MiB Yamux window.
  - `balanced/1` — `sched-w262144-q16384-cap65536`.
  - `trading/1` — `sched-w262144-q2048-cap8192`.
- Path: the product's `yamux_config` and the shipped relay (`relay_with_idle_timeout` for Baseline,
  `relay_with_policy` for the two profiles), both ends in one process, joined by a userspace
  rate-limiting proxy over loopback TCP. No TLS or Noise handshake, no NIC, no second host.
- Scenarios: `trading_idle` (8 s, no bulk), `trading_bulk_competing` (8 s per arm run, 3 repeats per
  arm), `trading_multi_flow` (2 and 3 bulk flows, first closed mid-run), ping interval 20 ms, and
  `relay_cpu_probe` at 128 MiB per repeat. The idle arm is new relative to TP-3.2's record: §6
  criterion 1 names idle latency and the harness can supply it per arm.
- Bottleneck rate 2,500,000 B/s (about 2.5 MB/s, the TP-2.2 rate).
- Environment: four-core Linux `4.19.0-gvisor` sandbox (`x86_64`), one process, no other load,
  2026-11-26.
- Run identity: run `tp-3.3-local-2026-11-26`, one local invocation at 19:52-19:55 UTC
  (176 s), `POLICY_REPEATS=3`, harness revision = this record's commit. `git diff 9372b45 <head> --
  src` touches only `#[cfg(test)] mod policy_baseline` (the shared evidence helper, the two ignored
  evidence tests and the additional pinning assertions), so the product code under test is identical
  to `main` at `9372b45`. The console lines that invocation printed are reproduced unedited in
  Appendix A.
- Every line below is labelled `simulated`. It is not `vm` and it is not `lab`.

## 2. Idle interactive RTT (`trading_idle`)

Microseconds; a 64-byte round trip on a session with no bulk stream. One run per arm (the scenario
collects about 300 samples in 8 s, so a repeat adds precision but not a new condition):

| Arm | samples | p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| baseline | 306 | 4,504 | 6,289 | 7,233 | 9,317 |
| `balanced/1` | 308 | 4,490 | 6,012 | 8,046 | 8,403 |
| `trading/1` | 307 | 4,505 | 5,833 | 6,778 | 8,027 |

Reading it honestly:

- All three arms sit within 15 µs at the median and within a few hundred microseconds at p95, i.e.
  the profile's 2 KiB quantum does not add measurable idle latency: the scheduler has nothing to
  arbitrate when one flow is alone.
- The p99 ordering (trading 6.8 ms < balanced 8.0 ms) is a single-run difference of a few samples,
  not a jitter advantage; it is not repeated and is not claimed as one.

## 3. Interactive RTT under bulk competition (`trading_bulk_competing`)

Microseconds; the ping shares the tunnel with one saturating bulk stream. One row per repeat;
medians across the three repeats are the row below each block.

| Arm | repeat | samples | p50 | p95 | p99 | max |
|---|---|---|---|---|---|---|
| baseline | 1 | 5 | 2,030,209 | 2,088,782 | 2,088,782 | 2,088,782 |
| baseline | 2 | 5 | 2,025,442 | 2,052,243 | 2,052,243 | 2,052,243 |
| baseline | 3 | 4 | 2,094,556 | 2,153,830 | 2,153,830 | 2,153,830 |
| **baseline (median)** | | | **2,030,209** | **2,088,782** | **2,088,782** | |
| `balanced/1` | 1 | 61 | 110,748 | 116,451 | 133,273 | 133,273 |
| `balanced/1` | 2 | 61 | 109,740 | 116,370 | 133,969 | 133,969 |
| `balanced/1` | 3 | 60 | 111,554 | 121,501 | 145,530 | 145,530 |
| **`balanced/1` (median)** | | | **110,748** | **116,451** | **133,969** | |
| `trading/1` | 1 | 60 | 112,275 | 119,288 | 161,472 | 161,472 |
| `trading/1` | 2 | 61 | 110,591 | 119,038 | 145,638 | 145,638 |
| `trading/1` | 3 | 60 | 111,423 | 119,653 | 147,716 | 147,716 |
| **`trading/1` (median)** | | | **111,423** | **119,288** | **147,716** | |

Reading it honestly:

- Both profile arms are roughly an order of magnitude below Baseline on every percentile
  (p50 2.03 s → about 0.11 s). That is the in-flight bound and the small per-flow queue, and it is
  reproduced in this harness, not on a real path.
- `trading/1` does **not** beat `balanced/1`: p50 +0.6%, p95 +2.4%, and p99 +10.3% at the median.
  Per repeat the p99 gap is +21.2%, +8.7% and +1.5%, i.e. Trading is 1.5-21.2% above Balanced in
  every repeat (trading 145,638-161,472 µs versus balanced 133,273-145,530 µs). So unlike Gaming it
  is not obviously noise, but it also is not a measured trading benefit — the finest quantum did
  not lower the loaded tail here.
- The Baseline arm collected only 4-5 samples per run because each ping waits out the queue, so its
  percentiles are coarse; the profile arms collected 60-61.
- "Jitter" here is the within-run p95−p50 / p99−p50 spread of the same samples. It is not a separate
  jitter measurement, and no propagation delay, loss or reordering is modelled.

## 4. Sender handoffs in the same runs (`trading_bulk_competing`)

Bytes the bulk sender handed to its application-side stream per requested second, same three
repeats. This is **not** delivered goodput: the harness counts writes into the application-side
duplex, starting from the bulk task's spawn (before the 500 ms pre-measurement sleep) and dividing
by the requested 8 s, so buffered data and the counted window both affect the figure. No sink-side
counter over an exact elapsed interval exists in this harness.

| Arm | repeat 1 | repeat 2 | repeat 3 | median |
|---|---|---|---|---|
| baseline | 3,155,968 | 3,155,968 | 2,631,680 | 3,155,968 |
| `balanced/1` | 2,156,544 | 2,156,544 | 2,123,776 | 2,156,544 |
| `trading/1` | 2,121,728 | 2,154,496 | 2,121,728 | 2,121,728 |

`trading/1` handed off 1.6% fewer bytes than `balanced/1` at the median and 33% fewer than
Baseline. The Baseline row's 3,155,968 B/s already exceeds the 2.5 MB/s shaped rate, which confirms
these are sender handoffs rather than bytes that crossed the bottleneck; no delivered-goodput or
bulk-share conclusion is drawn from them. The ADR describes Trading as "throughput deliberately
sacrificed"; at this rate the 2 KiB quantum does not show that sacrifice against Balanced in the
handoff count, and this harness cannot show it at higher rates or in delivered bytes. No throughput
benefit is shown either way. TP-3.2's record used the same counter under a "goodput" heading, so its
section 3 figures carry the same sender-side limitation.

## 5. Fairness and starvation (`trading_multi_flow`)

Three bulk flows share the bottleneck and the first is closed mid-run; the two survivors must keep
moving and share evenly (`delivery_min_over_max` compares the surviving flows only, so the two-flow
variant's 1.000 is trivial and is not used as evidence).

| Arm | flow bytes (closed, survivor, survivor) | echo p50 (µs), three flows | echo p95 (µs), three flows | split | continued after churn |
|---|---|---|---|---|---|
| baseline | 6,373,376 / 12,664,832 / 12,664,832 | 5,274,570 / 4,540,468 / 3,695,367 | 5,347,070 / 5,274,567 / 5,274,567 | 1.000 | yes |
| `balanced/1` | 3,096,576 / 6,897,664 / 6,897,664 | 250,754 / 261,168 / 261,166 | 373,648 / 376,937 / 376,936 | 1.000 | yes |
| `trading/1` | 2,949,120 / 6,881,280 / 6,881,280 | 252,093 / 252,092 / 252,092 | 382,746 / 382,745 / 382,745 | 1.000 | yes |

The bulk columns are indexed: position 0 is the flow closed at the churn point, positions 1 and 2
are the survivors, and `delivery_min_over_max` and `continued_after_churn` use those indices. The
echo columns are **not** indexed — the harness collects each flow's ping task in completion order
and prints no flow id, so those three figures are the three echo streams' percentiles and cannot be
attributed to a named survivor; every one of the three kept completing, and they are not used to
compare arms.

All arms kept both survivors moving, delivered the closed flow's bytes up to the close, split the
survivors evenly and reported no stall. No starvation and no fairness advantage over Balanced is
shown. The two-flow variant (not tabulated; split trivial) delivered 4,128,768 and 11,862,016 bytes
for `trading/1` against 4,276,224 and 11,878,400 for `balanced/1`, same shape, no stall.

## 6. CPU (`relay_cpu_probe`, unshaped loopback, 128 MiB per repeat)

Whole-process user+system CPU per delivered MiB, an upper bound on the relay's own cost because the
harness, both tunnel ends and the test app share the process:

| Arm | repeat 1 | repeat 2 | repeat 3 | median |
|---|---|---|---|---|
| baseline | 2,968.8 | 3,437.5 | 3,046.9 | 3,046.9 |
| `balanced/1` | 4,375.0 | 6,015.6 | 3,281.2 | 4,375.0 |
| `trading/1` | 19,531.2 | 19,140.6 | 17,968.8 | 19,140.6 |

`trading/1` costs about 4.4× `balanced/1` and 6.3× Baseline per MB at the median; every `trading/1`
repeat is more than twice the slowest `balanced/1` repeat. That is the price of a 2 KiB quantum and
an 8 KiB cap — eight times as many wake-ups per MB as Balanced — and it is a measured cost, not a
benefit. The absolute windows are short (2.3-2.5 s of CPU for the probe), so the per-MB figures are
approximate; the ordering is not.

## 7. Acceptance criteria, one by one

Against the list in [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) §6, for
`trading/1` versus Balanced and Baseline:

| # | Criterion | State here | Label |
|---|---|---|---|
| 1 | Latency p50/p95/p99 and jitter, idle and under bulk load | Idle RTT (section 2) and bulk-competing percentiles (section 3) measured in the harness; **real-path latency not measured** | `simulated` (partial) |
| 2 | Packet loss, or a loss proxy labelled as such | Not modelled: the shaper applies serialization rate only, no loss | **not measured** |
| 3 | Throughput / goodput | Sender handoff rate measured (section 4); **delivered goodput not measured** — the harness counts writes into the application-side duplex, not bytes at the sink over an exact interval | `simulated` (partial) |
| 4 | CPU and memory overhead | CPU per MB measured (section 6); **RSS not measured** — one process hosts every arm, so per-arm RSS is not separable | `simulated` (CPU only) |
| 5 | Fairness, no starvation | Three-flow share with a mid-run close (section 5) | `simulated` |
| 6 | Congestion / impairment (loss, jitter, path change, carrier kill) | Not modelled | **not measured** |
| 7 | Long run (60 minutes per policy) | Longest run 8 s per arm, 176 s for the whole test | **not measured** |
| 8 | Results published with an evidence label | This file; every number above is `simulated` | done for this slice |

## 8. Per-tuple claim (ADR-0047)

The claim belongs to `(engine, carrier, profile)`, and no engine inherits another's.

| Engine | Carrier | Profile | Claim | Why |
|---|---|---|---|---|
| `tcp-tls` | TCP/Yamux | `trading/1` | **`unknown`** | No run of this tuple. The harness runs the relay and Yamux over loopback TCP with no TLS handshake; its numbers are not this tuple's. Needs the two-host LAB window |
| `noise-tcp` | TCP/Yamux | `trading/1` | **`unknown`** | Same: no engine handshake, no real path |
| `baft` | TCP | `trading/1` | **`unsupported`** | Out of scope by ADR-0031 decision 17 |
| any | UDP/datagram | `trading/1` | **`unsupported`** (no carrier) | The datagram carrier is #244, not implemented. Any claim that depends on avoiding TCP head-of-line blocking needs it first (decision 11) |

The `simulated` harness numbers in sections 2-6 are a reference point for the future tuple runs, not
a claim for any of them. Section 4 is a sender-side count and is not usable as a delivered-throughput
claim even at that level.

## 9. What is still missing, precisely

The item's acceptance needs a real two-host LAB run — the suspended `baft-tunnel-lab` window —
covering, per `(engine, carrier, profile)` tuple: idle and bulked p50/p95/p99 latency and jitter, a
measured loss figure (or an explicit loss proxy), throughput, CPU/RSS, fairness under competing
bulk, impairment scenarios (loss, jitter, path change, carrier kill), and a run up to the
60-minute project cap. Until that window is authorized, TP-3.3 stays open, no tuple becomes
`measured`, and nothing here may be quoted as a Trading benefit. One trading-specific question the
LAB should answer, because this harness raises it: whether the 4.4× CPU cost per MB and the
consistently higher p99 survive at real rates, and whether Trading is worth keeping when it does not
beat Balanced even in the harness.

## 10. Reproduce

```
# at this record's commit (or any later commit that keeps the test)
POLICY_REPEATS=3 cargo test --release --features policy-pack -- \
  --ignored --nocapture trading_profile_evidence
```

Each `POLICY_EXPERIMENT …` line is one arm run, JSON after the first object, all labelled
`simulated`. `POLICY_RATE`, `POLICY_SECONDS` and `POLICY_BYTES` change the rate, the per-arm
duration and the CPU probe size; the defaults are the figures above. Use a fresh target directory
in this sandbox (`/tmp` is 9p and already tight); the run takes about three minutes in release.

## Appendix A. Raw run output, unedited

The 27 `POLICY_EXPERIMENT` lines run `tp-3.3-local-2026-11-26` printed (3 idle, 9 bulk competing,
6 multi-flow, 9 CPU probe), followed by the test summary. `Mechanism::label()` prints the resolved
parameters, so `sched-w262144-q16384-cap65536` is `balanced/1`'s arm and
`sched-w262144-q2048-cap8192` is `trading/1`'s; `baseline` is the pack off.

```
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1} {"scenario":"trading_idle","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":306,"rtt_us":{"p50":4504,"p95":6289,"p99":7233,"max":9317},"bulk_bytes":0,"cpu_us":680000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1} {"scenario":"trading_idle","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":308,"rtt_us":{"p50":4490,"p95":6012,"p99":8046,"max":8403},"bulk_bytes":0,"cpu_us":560000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1} {"scenario":"trading_idle","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","rate_bytes_per_s":2500000,"samples":307,"rtt_us":{"p50":4505,"p95":5833,"p99":6778,"max":8027},"bulk_bytes":0,"cpu_us":610000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"bulk_bytes_per_s":3155968} {"scenario":"trading_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":5,"rtt_us":{"p50":2030209,"p95":2088782,"p99":2088782,"max":2088782},"bulk_bytes":25247744,"cpu_us":1140000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":2,"bulk_bytes_per_s":3155968} {"scenario":"trading_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":5,"rtt_us":{"p50":2025442,"p95":2052243,"p99":2052243,"max":2052243},"bulk_bytes":25247744,"cpu_us":1270000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":3,"bulk_bytes_per_s":2631680} {"scenario":"trading_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":4,"rtt_us":{"p50":2094556,"p95":2153830,"p99":2153830,"max":2153830},"bulk_bytes":21053440,"cpu_us":990000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"bulk_bytes_per_s":2156544} {"scenario":"trading_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":61,"rtt_us":{"p50":110748,"p95":116451,"p99":133273,"max":133273},"bulk_bytes":17252352,"cpu_us":1050000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":2,"bulk_bytes_per_s":2156544} {"scenario":"trading_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":61,"rtt_us":{"p50":109740,"p95":116370,"p99":133969,"max":133969},"bulk_bytes":17252352,"cpu_us":1120000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":3,"bulk_bytes_per_s":2123776} {"scenario":"trading_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":111554,"p95":121501,"p99":145530,"max":145530},"bulk_bytes":16990208,"cpu_us":1230000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"bulk_bytes_per_s":2121728} {"scenario":"trading_bulk_competing","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":112275,"p95":119288,"p99":161472,"max":161472},"bulk_bytes":16973824,"cpu_us":1400000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":2,"bulk_bytes_per_s":2154496} {"scenario":"trading_bulk_competing","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","rate_bytes_per_s":2500000,"samples":61,"rtt_us":{"p50":110591,"p95":119038,"p99":145638,"max":145638},"bulk_bytes":17235968,"cpu_us":1240000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":3,"bulk_bytes_per_s":2121728} {"scenario":"trading_bulk_competing","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":111423,"p95":119653,"p99":147716,"max":147716},"bulk_bytes":16973824,"cpu_us":1260000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"flows":2} {"scenario":"trading_multi_flow","mechanism":"baseline","evidence":"simulated","flows":2,"bulk_bytes":[6373376,16859136],"total_bulk_bytes":23232512,"delivery_min_over_max":1.000,"flow_p50_us":[2080431,2080430],"flow_p95_us":[3419842,3156660],"cpu_us":1110000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"flows":2} {"scenario":"trading_multi_flow","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","flows":2,"bulk_bytes":[4276224,11878400],"total_bulk_bytes":16154624,"delivery_min_over_max":1.000,"flow_p50_us":[114353,114385],"flow_p95_us":[248436,250104],"cpu_us":1080000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"flows":2} {"scenario":"trading_multi_flow","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","flows":2,"bulk_bytes":[4128768,11862016],"total_bulk_bytes":15990784,"delivery_min_over_max":1.000,"flow_p50_us":[114282,114282],"flow_p95_us":[248863,248862],"cpu_us":1160000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"flows":3} {"scenario":"trading_multi_flow","mechanism":"baseline","evidence":"simulated","flows":3,"bulk_bytes":[6373376,12664832,12664832],"total_bulk_bytes":31703040,"delivery_min_over_max":1.000,"flow_p50_us":[5274570,4540468,3695367],"flow_p95_us":[5347070,5274567,5274567],"cpu_us":1310000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"flows":3} {"scenario":"trading_multi_flow","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","flows":3,"bulk_bytes":[3096576,6897664,6897664],"total_bulk_bytes":16891904,"delivery_min_over_max":1.000,"flow_p50_us":[250754,261168,261166],"flow_p95_us":[373648,376937,376936],"cpu_us":1060000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","repeat":1,"flows":3} {"scenario":"trading_multi_flow","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","flows":3,"bulk_bytes":[2949120,6881280,6881280],"total_bulk_bytes":16711680,"delivery_min_over_max":1.000,"flow_p50_us":[252093,252092,252092],"flow_p95_us":[382746,382745,382745],"cpu_us":960000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":380000,"cpu_us_per_mb":2968.8}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":440000,"cpu_us_per_mb":3437.5}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":390000,"cpu_us_per_mb":3046.9}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":560000,"cpu_us_per_mb":4375.0}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":770000,"cpu_us_per_mb":6015.6}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":420000,"cpu_us_per_mb":3281.2}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":2500000,"cpu_us_per_mb":19531.2}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":2450000,"cpu_us_per_mb":19140.6}
POLICY_EXPERIMENT {"profile_run":"tp-3.3","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q2048-cap8192","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":2300000,"cpu_us_per_mb":17968.8}
```

Test summary: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 480 filtered out;
finished in 176.09s`.
