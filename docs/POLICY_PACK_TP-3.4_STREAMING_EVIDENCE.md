# TP-3.4 Streaming profile: per-tuple evidence record

**Status: `(tcp-tls, TCP/Yamux, `streaming/1`)` is `measured` on the two-host LAB (2026-11-28, run 46150559605); every other tuple is unmeasured and the item stays open.**
Item: TP-3.4 (#236), `streaming/1`. Decision: [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md)
decisions 15-20 and its TP-3.4 implementation note. Acceptance list:
[ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) §6. Status contract:
[ADR-0047](ADR-0047-policy-status-contract.md).

This record exists to keep the claim honest, not to close the item. The row already resolved through
the TP-3.1 `policy.profile` path before this item; what TP-3.4 adds is this record, the tests that pin
the row against the shipped table, and an explicit statement of what the repository harness can and
cannot measure. **No benefit is claimed and no profile is `measured`.**

## 1. What was run

- Harness: `src/policy_baseline.rs`, test `policy_baseline::tests::streaming_profile_evidence`
  (`cargo test --release --features policy-pack -- --ignored --nocapture streaming_profile_evidence`).
- Arms, generated from the shipped table by test-only `Mechanism::for_profile` (not hand-copied
  numbers; `profile_arms_are_the_shipped_rows` asserts the mapping, now including the `streaming/1`
  literal):
  - `baseline` — the pack off, today's relay pump and today's 4 MiB Yamux window.
  - `balanced/1` — `sched-w262144-q16384-cap65536`.
  - `streaming/1` — `sched-w524288-q32768-cap131072` (512 KiB window, 32 KiB quantum, 128 KiB cap).
- Path: the product's `yamux_config` and the shipped relay (`relay_with_idle_timeout` for Baseline,
  `relay_with_policy` for the two profiles), both ends in one process, joined by a userspace
  rate-limiting proxy over loopback TCP. No TLS or Noise handshake, no NIC, no second host.
- Scenarios: `streaming_idle` (8 s, no bulk), `streaming_bulk_competing` (8 s per arm run, 3 repeats
  per arm), `streaming_multi_flow` (2 and 3 bulk flows, first closed mid-run), ping interval 20 ms,
  and `relay_cpu_probe` at 128 MiB per repeat. The idle arm follows TP-3.3's record: §6 criterion 1
  names idle latency and the harness can supply it per arm.
- Bottleneck rate 2,500,000 B/s (about 2.5 MB/s, the TP-2.2 rate).
- Environment: four-core Linux `4.19.0-gvisor` sandbox (`x86_64`), one process, no other load,
  2026-11-26.
- Run identity: run `tp-3.4-local-2026-11-26`, one local invocation on 2026-11-26 at about
  20:44-20:47 UTC (168 s), `POLICY_REPEATS=3`,
  **harness revision `ad691d82da76ce6f4a50d842d80204306ead7dee`** (this record's first commit, whose
  `src` diff is exactly the tree the invocation ran; no other uncommitted source change was present.
  Its review-fix follow-up changes only this document). Its `git diff 48b69cf1bee49f6a0ee4a2fd4d64e659e9ae9560 -- src` touches only
  `#[cfg(test)] mod policy_baseline` (the ignored evidence test and the additional pinning
  assertions), so the **product code under test is identical to `main` at
  `48b69cf1bee49f6a0ee4a2fd4d64e659e9ae9560`**. The console lines that invocation printed are
  reproduced unedited in Appendix A.
- Every line below is labelled `simulated`. It is not `vm` and it is not `lab`.

## 2. Idle interactive RTT (`streaming_idle`)

Microseconds; a 64-byte round trip on a session with no bulk stream. One run per arm (the scenario
collects about 300 samples in 8 s, so a repeat adds precision but not a new condition):

| Arm | samples | p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| baseline | 307 | 4,492 | 6,105 | 7,597 | 10,181 |
| `balanced/1` | 306 | 4,490 | 5,718 | 7,466 | 11,481 |
| `streaming/1` | 304 | 4,538 | 6,610 | 8,947 | 17,020 |

Reading it honestly:

- At the median the three arms are within 48 µs (streaming is +1.1% over Balanced), i.e. the wider
  window and 32 KiB quantum add no measurable idle latency: the scheduler has nothing to arbitrate
  when one flow is alone.
- The tail figures are single-run differences of a few samples (`streaming/1` p99 8.9 ms, max 17.0 ms
  against Balanced's 7.5 ms / 11.5 ms); they are not repeated and are not claimed as a jitter
  advantage or penalty.

## 3. Interactive RTT under bulk competition (`streaming_bulk_competing`)

Microseconds; the ping shares the tunnel with one saturating bulk stream. One row per repeat;
medians across the three repeats are the row below each block.

| Arm | repeat | samples | p50 | p95 | p99 | max |
|---|---|---|---|---|---|---|
| baseline | 1 | 5 | 2,071,929 | 2,100,454 | 2,100,454 | 2,100,454 |
| baseline | 2 | 5 | 2,080,296 | 2,098,333 | 2,098,333 | 2,098,333 |
| baseline | 3 | 4 | 2,069,155 | 2,127,765 | 2,127,765 | 2,127,765 |
| **baseline (median)** | | | **2,071,929** | **2,100,454** | **2,100,454** | |
| `balanced/1` | 1 | 62 | 109,306 | 116,154 | 118,088 | 118,088 |
| `balanced/1` | 2 | 62 | 107,643 | 110,961 | 115,773 | 115,773 |
| `balanced/1` | 3 | 62 | 107,801 | 113,353 | 114,994 | 114,994 |
| **`balanced/1` (median)** | | | **107,801** | **113,353** | **115,773** | |
| `streaming/1` | 1 | 32 | 235,461 | 242,617 | 243,121 | 243,121 |
| `streaming/1` | 2 | 32 | 235,445 | 242,375 | 244,013 | 244,013 |
| `streaming/1` | 3 | 32 | 236,006 | 242,627 | 243,655 | 243,655 |
| **`streaming/1` (median)** | | | **235,461** | **242,617** | **243,655** | |

Reading it honestly:

- Both profile arms are far below Baseline on every percentile (p50 2.07 s → 0.11-0.24 s). That is
  the in-flight bound, reproduced in this harness, not on a real path.
- `streaming/1` is **worse than `balanced/1` on latency under competition, by design and
  consistently**: p50 +118.4%, p95 +114.0%, p99 +110.5% at the medians, and the per-repeat p50 gap is
  +115.4%, +118.7% and +118.9% — never less than double. The row holds twice Balanced's window and
  cap, so a competing interactive ping waits behind a larger standing buffer; the ADR describes that
  as "a larger but still bounded standing buffer". This record measures that cost; it shows no
  latency benefit and none is claimed.
- `streaming/1` still keeps loaded p50 at 8.8× below Baseline, and its 32 samples per run are enough
  that the gap is not a sample-count artefact, but the percentile tails are coarse (the profile arms
  collected 32 samples, Baseline 4-5 because each ping waits out the queue).
- "Jitter" here is the within-run p95−p50 / p99−p50 spread of the same samples. It is not a separate
  jitter measurement, and no propagation delay, loss or reordering is modelled.

## 4. Sender handoffs in the same runs (`streaming_bulk_competing`)

Bytes the bulk sender handed to its application-side stream per requested second, same three
repeats. This is **not** delivered goodput: the harness counts writes into the application-side
duplex, starting from the bulk task's spawn (before the 500 ms pre-measurement sleep) and dividing by
the requested 8 s, so buffered data and the counted window both affect the figure. No sink-side
counter over an exact elapsed interval exists in this harness.

| Arm | repeat 1 | repeat 2 | repeat 3 | median |
|---|---|---|---|---|
| baseline | 3,155,968 | 3,155,968 | 2,631,680 | 3,155,968 |
| `balanced/1` | 2,189,312 | 2,189,312 | 2,189,312 | 2,189,312 |
| `streaming/1` | 2,273,280 | 2,273,280 | 2,273,280 | 2,273,280 |

`streaming/1` handed off 3.8% more bytes than `balanced/1` at the median and 28% fewer than
Baseline. The Baseline row's 3,155,968 B/s already exceeds the 2.5 MB/s shaped rate, which confirms
these are sender handoffs rather than bytes that crossed the bottleneck; no delivered-goodput or
bulk-share conclusion is drawn from them. A higher handoff count at a larger quantum is consistent
with the profile's stated purpose (sustained delivery with a larger standing buffer), but this
harness cannot turn it into a delivered-throughput claim. TP-3.2's and TP-3.3's records used the same
counter; TP-3.2 titled it "goodput", so its section 3 figures carry the same sender-side limitation.

## 5. Fairness and starvation (`streaming_multi_flow`)

Three bulk flows share the bottleneck and the first is closed mid-run; the two survivors must keep
handing off bytes and share evenly (`delivery_min_over_max` compares the surviving flows' handoff
counters only, so the two-flow variant's 1.000 is trivial and is not used as evidence).

| Arm | flow bytes (closed, survivor, survivor) | echo p50 (µs), three flows | echo p95 (µs), three flows | split | continued after churn |
|---|---|---|---|---|---|
| baseline | 6,373,376 / 12,664,832 / 12,664,832 | 5,175,214 / 4,469,802 / 3,668,202 | 5,262,782 / 5,175,212 / 5,175,212 | 1.000 | yes |
| `balanced/1` | 3,096,576 / 6,897,664 / 6,897,664 | 255,258 / 262,903 / 262,902 | 373,199 / 375,540 / 375,539 | 1.000 | yes |
| `streaming/1` | 3,244,032 / 7,438,336 / 7,438,336 | 507,173 / 535,026 / 535,024 | 801,669 / 791,436 / 791,435 | 1.000 | yes |

The bulk columns are indexed: position 0 is the flow closed at the churn point, positions 1 and 2 are
the survivors, and `delivery_min_over_max` and `continued_after_churn` use those indices; the counts
are application-side handoffs, not sink-side delivery. The echo columns are **not** indexed — the
harness collects each flow's ping task in completion order and prints no flow id, so those three
figures are the three echo streams' percentiles and cannot be attributed to a named survivor; every
one of the three kept completing, and they are not used to compare arms.

All arms kept both survivors moving, handed off the closed flow's bytes up to the close, split the
survivors evenly and reported no stall. `streaming/1` handed off 7.3% more total bytes than
`balanced/1` in the same 8 s (18,120,704 against 16,891,904 across the three flows, sender side) and
its echoes under three-flow competition are about twice Balanced's p50 — the same buffer cost as
section 3. No starvation and no fairness advantage over Balanced is shown. The two-flow variant (not
tabulated; split trivial) handed off 4,816,896 and 12,156,928 bytes for `streaming/1` against
4,407,296 and 12,271,616 for `balanced/1`, same shape, no stall.

## 6. CPU (`relay_cpu_probe`, unshaped loopback, 128 MiB per repeat)

Whole-process user+system CPU per delivered MiB, an upper bound on the relay's own cost because the
harness, both tunnel ends and the test app share the process:

| Arm | repeat 1 | repeat 2 | repeat 3 | median |
|---|---|---|---|---|
| baseline | 3,593.8 | 3,515.6 | 4,375.0 | 3,593.8 |
| `balanced/1` | 3,828.1 | 5,625.0 | 4,687.5 | 4,687.5 |
| `streaming/1` | 2,890.6 | 2,968.8 | 2,734.4 | 2,890.6 |

`streaming/1` costs 38.3% less CPU per delivered MB than `balanced/1` and 19.6% less than Baseline
at the medians, and the ordering holds across repeats: every `streaming/1` repeat (2,734-2,969) is
below every `balanced/1` repeat (3,828-5,625). That is what fewer wake-ups per MB at a 32 KiB quantum
would predict, and it is a measured cost *reduction*, not a throughput benefit. The absolute windows
are very short (0.35-0.72 s of CPU for the 128 MiB probe, i.e. `cpu_us` 350,000-720,000 µs) and the
harness's own per-run spread is large
(`balanced/1` spans 1,797 µs/MB between its repeats), and Baseline's median here is 18% above its
3,046.9 in the TP-3.3 run — so compare within this invocation only; the between-arm ordering is the
usable part.

## 7. Acceptance criteria, one by one

Against the list in [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) §6, for
`streaming/1` versus Balanced and Baseline:

| # | Criterion | State here | Label |
|---|---|---|---|
| 1 | Latency p50/p95/p99 and jitter, idle and under bulk load | Idle RTT (section 2) and bulk-competing percentiles (section 3) measured in the harness; **real-path latency not measured** | `simulated` (partial) |
| 2 | Packet loss, or a loss proxy labelled as such | Not modelled: the shaper applies serialization rate only, no loss | **not measured** |
| 3 | Throughput / goodput | Sender handoff rate measured (section 4); **delivered goodput not measured** — the harness counts writes into the application-side duplex, not bytes at the sink over an exact interval | `simulated` (partial) |
| 4 | CPU and memory overhead | CPU per MB measured (section 6); **RSS not measured** — one process hosts every arm, so per-arm RSS is not separable | `simulated` (CPU only) |
| 5 | Fairness, no starvation | Three-flow share with a mid-run close (section 5) | `simulated` |
| 6 | Congestion / impairment (loss, jitter, path change, carrier kill) | Not modelled | **not measured** |
| 7 | Long run (60 minutes per policy) | Longest run 8 s per arm, 168 s for the whole test | **not measured** |
| 8 | Results published with an evidence label | This file; every number above is `simulated` | done for this slice |

## 8. Per-tuple claim (ADR-0047)

The claim belongs to `(engine, carrier, profile)`, and no engine inherits another's.

| Engine | Carrier | Profile | Claim | Why |
|---|---|---|---|---|
| `tcp-tls` | TCP/Yamux | `streaming/1` | **`unknown`** | No run of this tuple. The harness runs the relay and Yamux over loopback TCP with no TLS handshake; its numbers are not this tuple's. Needs the two-host LAB window |
| `noise-tcp` | TCP/Yamux | `streaming/1` | **`unknown`** | Same: no engine handshake, no real path |
| `baft` | TCP | `streaming/1` | **`unsupported`** | Out of scope by ADR-0031 decision 17 |
| any | UDP/datagram | `streaming/1` | **`unsupported`** (no carrier) | The datagram carrier is #244, not implemented. Streaming is not listed as needing it (ADR-0031 decision 11 / pack plan amendment), but no datagram tuple exists |

The `simulated` harness numbers in sections 2-6 are a reference point for the future tuple runs, not
a claim for any of them. Sections 4 and 5 are sender-side counts and are not usable as
delivered-throughput claims even at that level.

## 9. What is still missing, precisely

The item's acceptance needs a real two-host LAB run — the suspended `baft-tunnel-lab` window —
covering, per `(engine, carrier, profile)` tuple: idle and bulked p50/p95/p99 latency and jitter, a
measured loss figure (or an explicit loss proxy), throughput, CPU/RSS, fairness under competing
bulk, impairment scenarios (loss, jitter, path change, carrier kill), and a run up to the
60-minute project cap. Until that window is authorized, TP-3.4 stays open, no tuple becomes
`measured`, and nothing here may be quoted as a Streaming benefit. Questions this harness puts to
that window, because it cannot answer them: whether the 38% lower CPU per MB survives at real rates
with a real engine handshake, whether the sender-handoff advantage becomes delivered goodput on a
path with real RTT and loss, and whether the roughly doubled loaded p50 is acceptable for the
"sustained medium-bitrate" use the row targets when a stream competes with bulk.

## 10. Reproduce

```
# at this record's commit (or any later commit that keeps the test)
POLICY_REPEATS=3 cargo test --release --features policy-pack -- \
  --ignored --nocapture streaming_profile_evidence
```

Each `POLICY_EXPERIMENT …` line is one arm run, JSON after the first object, all labelled
`simulated`. `POLICY_RATE`, `POLICY_SECONDS` and `POLICY_BYTES` change the rate, the per-arm
duration and the CPU probe size; the defaults are the figures above. Use a fresh target directory
in this sandbox (`/tmp` is 9p and already tight); the run takes about three minutes in release.

## Appendix A. Raw run output, unedited

The 27 `POLICY_EXPERIMENT` lines run `tp-3.4-local-2026-11-26` printed (3 idle, 9 bulk competing,
6 multi-flow, 9 CPU probe), followed by the test summary. `Mechanism::label()` prints the resolved
parameters, so `sched-w262144-q16384-cap65536` is `balanced/1`'s arm and
`sched-w524288-q32768-cap131072` is `streaming/1`'s; `baseline` is the pack off.

```
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1} {"scenario":"streaming_idle","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":307,"rtt_us":{"p50":4492,"p95":6105,"p99":7597,"max":10181},"bulk_bytes":0,"cpu_us":580000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1} {"scenario":"streaming_idle","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":306,"rtt_us":{"p50":4490,"p95":5718,"p99":7466,"max":11481},"bulk_bytes":0,"cpu_us":460000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1} {"scenario":"streaming_idle","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","rate_bytes_per_s":2500000,"samples":304,"rtt_us":{"p50":4538,"p95":6610,"p99":8947,"max":17020},"bulk_bytes":0,"cpu_us":700000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"bulk_bytes_per_s":3155968} {"scenario":"streaming_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":5,"rtt_us":{"p50":2071929,"p95":2100454,"p99":2100454,"max":2100454},"bulk_bytes":25247744,"cpu_us":1280000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":2,"bulk_bytes_per_s":3155968} {"scenario":"streaming_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":5,"rtt_us":{"p50":2080296,"p95":2098333,"p99":2098333,"max":2098333},"bulk_bytes":25247744,"cpu_us":1170000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":3,"bulk_bytes_per_s":2631680} {"scenario":"streaming_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":4,"rtt_us":{"p50":2069155,"p95":2127765,"p99":2127765,"max":2127765},"bulk_bytes":21053440,"cpu_us":1000000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"bulk_bytes_per_s":2189312} {"scenario":"streaming_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":62,"rtt_us":{"p50":109306,"p95":116154,"p99":118088,"max":118088},"bulk_bytes":17514496,"cpu_us":1150000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":2,"bulk_bytes_per_s":2189312} {"scenario":"streaming_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":62,"rtt_us":{"p50":107643,"p95":110961,"p99":115773,"max":115773},"bulk_bytes":17514496,"cpu_us":1010000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":3,"bulk_bytes_per_s":2189312} {"scenario":"streaming_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":62,"rtt_us":{"p50":107801,"p95":113353,"p99":114994,"max":114994},"bulk_bytes":17514496,"cpu_us":1030000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"bulk_bytes_per_s":2273280} {"scenario":"streaming_bulk_competing","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","rate_bytes_per_s":2500000,"samples":32,"rtt_us":{"p50":235461,"p95":242617,"p99":243121,"max":243121},"bulk_bytes":18186240,"cpu_us":1090000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":2,"bulk_bytes_per_s":2273280} {"scenario":"streaming_bulk_competing","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","rate_bytes_per_s":2500000,"samples":32,"rtt_us":{"p50":235445,"p95":242375,"p99":244013,"max":244013},"bulk_bytes":18186240,"cpu_us":1050000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":3,"bulk_bytes_per_s":2273280} {"scenario":"streaming_bulk_competing","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","rate_bytes_per_s":2500000,"samples":32,"rtt_us":{"p50":236006,"p95":242627,"p99":243655,"max":243655},"bulk_bytes":18186240,"cpu_us":1010000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"flows":2} {"scenario":"streaming_multi_flow","mechanism":"baseline","evidence":"simulated","flows":2,"bulk_bytes":[6373376,16859136],"total_bulk_bytes":23232512,"delivery_min_over_max":1.000,"flow_p50_us":[2010278,2010276],"flow_p95_us":[3327768,3084836],"cpu_us":1200000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"flows":2} {"scenario":"streaming_multi_flow","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","flows":2,"bulk_bytes":[4407296,12271616],"total_bulk_bytes":16678912,"delivery_min_over_max":1.000,"flow_p50_us":[109680,109699],"flow_p95_us":[241676,240783],"cpu_us":910000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"flows":2} {"scenario":"streaming_multi_flow","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","flows":2,"bulk_bytes":[4816896,12156928],"total_bulk_bytes":16973824,"delivery_min_over_max":1.000,"flow_p50_us":[274756,237885],"flow_p95_us":[493181,497407],"cpu_us":1040000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"flows":3} {"scenario":"streaming_multi_flow","mechanism":"baseline","evidence":"simulated","flows":3,"bulk_bytes":[6373376,12664832,12664832],"total_bulk_bytes":31703040,"delivery_min_over_max":1.000,"flow_p50_us":[5175214,4469802,3668202],"flow_p95_us":[5262782,5175212,5175212],"cpu_us":1070000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"flows":3} {"scenario":"streaming_multi_flow","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","flows":3,"bulk_bytes":[3096576,6897664,6897664],"total_bulk_bytes":16891904,"delivery_min_over_max":1.000,"flow_p50_us":[255258,262903,262902],"flow_p95_us":[373199,375540,375539],"cpu_us":1050000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","repeat":1,"flows":3} {"scenario":"streaming_multi_flow","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","flows":3,"bulk_bytes":[3244032,7438336,7438336],"total_bulk_bytes":18120704,"delivery_min_over_max":1.000,"flow_p50_us":[507173,535026,535024],"flow_p95_us":[801669,791436,791435],"cpu_us":940000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":460000,"cpu_us_per_mb":3593.8}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":450000,"cpu_us_per_mb":3515.6}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":560000,"cpu_us_per_mb":4375.0}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":490000,"cpu_us_per_mb":3828.1}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":720000,"cpu_us_per_mb":5625.0}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":600000,"cpu_us_per_mb":4687.5}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":370000,"cpu_us_per_mb":2890.6}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":380000,"cpu_us_per_mb":2968.8}
POLICY_EXPERIMENT {"profile_run":"tp-3.4","scenario":"relay_cpu_probe","mechanism":"sched-w524288-q32768-cap131072","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":350000,"cpu_us_per_mb":2734.4}
```

Test summary: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 483 filtered out;
finished in 168.29s`.

## LAB measurement of this profile on the real path (2026-11-28)

Run [46150559605](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/46150559605) on `main` `d6dcf2c` is `success`. `lab/real-hosts/policy-lab-stage.sh` drove
the two LAB hosts for six arms under `.github/workflows/policy-lab.yml`, which builds the binary
from the dispatched `main` commit with `--features policy-pack`. This profile's arm was:

```
POLICY_LAB_ARM arm=streaming-1 profile=streaming/1 pack=sched pack_engaged=1 engine=tcp-tls carrier=tcp
  bytes=134217728 delivered_bytes=134217728 sink_MiB_per_s=3.47 interactive_p50_us=109,157 interactive_p95_us=189,844
  probe_samples=209 probe_bulk_window=True sha_match=1 evidence=LAB
  policy pack engaged: profile=streaming/1 window_bytes=524288 quantum_bytes=32768 max_queued_bytes=131072 adaptive=off
```

What that is: 128 MiB of a deterministic stream that the sink, behind the tunnel on the abroad
host, hashed byte-exact (`sha_match=1`); a sink-clocked throughput of **3.47 MiB/s**; and 64-byte
echo round trips from the Iran host, sampled **only inside the bulk window** (209 samples, p50
**109,157 µs**, p95 **189,844 µs**), so the two flows competed for one tunnel session. The pack's own
`policy pack engaged` line appeared on **both** hosts with the row above and `adaptive=off`; the
control arm in the same run engaged nothing. For scale in that same run: control 19.44 MiB/s and
125,836 µs p50; `balanced/1` 1.82 MiB/s and 110,634 µs.

So `(tcp-tls, TCP/Yamux, streaming/1)` is **`measured` for this path and window**, once — one run,
so the run-to-run spread is unknown. Still **unmeasured**, and the reasons the item stays open:
`(noise-tcp, TCP, streaming/1)`; the UDP/datagram carrier, which has no arm in this harness at all
(**NOT TESTED**, not approximated); loss and jitter/impairment (no shaper on this path); RSS; the
60-minute run; repeat runs for a spread; and the adaptive controller, which was off in every arm.
No benefit is claimed for this row from this run: the scheduler rows delivered markedly less bulk
goodput than the control on this path.
