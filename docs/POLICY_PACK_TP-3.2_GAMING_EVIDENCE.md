# TP-3.2 Gaming profile: per-tuple evidence record

**Status: `(tcp-tls, TCP/Yamux, `gaming/1`)` is `measured` on the two-host LAB (2026-11-28, run 46150559605); every other tuple is unmeasured and the item stays open.**
Item: TP-3.2 (#234), `gaming/1`. Decision: [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md)
decisions 15-20 and its TP-3.2 implementation note. Acceptance list:
[ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) §6. Status contract:
[ADR-0047](ADR-0047-policy-status-contract.md).

This record exists to keep the claim honest, not to close the item. It contains what the repository
harness can measure on a single host, labelled `simulated`, and it names everything else as not
measured. **No benefit is claimed and no profile is `measured`.**

## 1. What was run

- Harness: `src/policy_baseline.rs`, test `policy_baseline::tests::gaming_profile_evidence`
  (`cargo test --release --features policy-pack -- --ignored --nocapture gaming_profile_evidence`).
- Arms, generated from the shipped table by test-only `Mechanism::for_profile` (not hand-copied
  numbers; `profile_arms_are_the_shipped_rows` asserts the mapping):
  - `baseline` — the pack off, today's relay pump and today's 4 MiB Yamux window.
  - `balanced/1` — `sched-w262144-q16384-cap65536`.
  - `gaming/1` — `sched-w262144-q4096-cap16384`.
- Path: the product's `yamux_config` and the shipped relay (`relay_with_idle_timeout` for Baseline,
  `relay_with_policy` for the two profiles), both ends in one process, joined by a userspace
  rate-limiting proxy over loopback TCP. No TLS or Noise handshake, no NIC, no second host.
- Bottleneck rate 2,500,000 B/s (about 2.5 MB/s, the TP-2.2 rate), 8 s per arm run, 3 repeats per
  arm, ping interval 20 ms, 128 MiB per CPU probe.
- Environment: four-core Linux `4.19.0-gvisor` sandbox (`x86_64`), one process, no other load,
  2026-11-26.
- Run identity: run `tp-3.2-local-2026-11-26`, one local invocation at 18:59 UTC with
  `POLICY_REPEATS=3`, harness revision `233e440c` (the commit this record ships in).
  `git diff 06aa420 233e440 -- src` is 152 added lines inside `#[cfg(test)] mod policy_baseline`
  only, so the product code under test is identical to `main` at `06aa420`. The console lines that
  invocation printed are reproduced unedited in Appendix A.
- Every line below is labelled `simulated`. It is not `vm` and it is not `lab`.

## 2. Interactive RTT under bulk competition (`gaming_bulk_competing`)

Microseconds; the ping is a 64-byte round trip that shares the tunnel with one saturating bulk
stream. One row per repeat; medians across the three repeats are the row below each block.

| Arm | repeat | samples | p50 | p95 | p99 | max |
|---|---|---|---|---|---|---|
| baseline | 1 | 4 | 2,090,030 | 2,147,812 | 2,147,812 | 2,147,812 |
| baseline | 2 | 4 | 2,080,693 | 2,136,313 | 2,136,313 | 2,136,313 |
| baseline | 3 | 4 | 2,101,010 | 2,136,034 | 2,136,034 | 2,136,034 |
| **baseline (median)** | | | **2,090,030** | **2,136,313** | **2,136,034** | |
| `balanced/1` | 1 | 60 | 111,982 | 118,325 | 123,807 | 123,807 |
| `balanced/1` | 2 | 60 | 112,642 | 120,149 | 124,870 | 124,870 |
| `balanced/1` | 3 | 60 | 111,575 | 118,709 | 122,755 | 122,755 |
| **`balanced/1` (median)** | | | **111,982** | **118,709** | **123,807** | |
| `gaming/1` | 1 | 61 | 110,934 | 117,617 | 119,438 | 119,438 |
| `gaming/1` | 2 | 60 | 112,113 | 118,788 | 128,400 | 128,400 |
| `gaming/1` | 3 | 62 | 108,700 | 116,492 | 117,837 | 117,837 |
| **`gaming/1` (median)** | | | **110,934** | **117,617** | **119,438** | |

Reading it honestly:

- Both profile arms are roughly an order of magnitude below Baseline on every percentile
  (p50 2.09 s → about 0.11 s). That is the in-flight bound and the small per-flow queue, and it is
  reproduced in this harness, not on a real path.
- `gaming/1` and `balanced/1` are **indistinguishable in this scenario**: p50 −0.9%, p95 −0.9%,
  p99 −3.5% against Balanced, all smaller than the spread between repeats of the same arm. The
  harness does not demonstrate a jitter advantage for the 4 KiB quantum at this rate.
- The Baseline arm collected only 4 samples per run because each ping waits out the queue, so its
  percentiles are coarse; the profile arms collected 60-62.
- "Jitter" here is the within-run p95−p50 / p99−p50 spread of the same samples. It is not a separate
  jitter measurement, and no propagation delay, loss or reordering is modelled.

## 3. Bulk goodput in the same runs

Bytes per second handed to the engine by the bulk flow, same three repeats:

| Arm | repeat 1 | repeat 2 | repeat 3 | median |
|---|---|---|---|---|
| baseline | 2,631,680 | 2,631,680 | 2,631,680 | 2,631,680 |
| `balanced/1` | 2,123,776 | 2,123,776 | 2,123,776 | 2,123,776 |
| `gaming/1` | 2,154,496 | 2,121,728 | 2,187,264 | 2,154,496 |

`gaming/1` carried 1.4% more bulk than `balanced/1` at the median and 18% less than Baseline; the
shaped rate is the same in every arm, so this is the share the arm's queueing left for bulk, not a
capacity change. No throughput benefit over Balanced is shown.

## 4. Fairness and starvation (`gaming_multi_flow`)

Three bulk flows share the bottleneck and the first is closed mid-run; the two survivors must keep
moving and share evenly (`delivery_min_over_max` compares the surviving flows only, so the two-flow
variant's 1.000 is trivial and is not used as evidence).

| Arm | flow bytes (closed, survivor, survivor) | echo p50 (µs), three flows | echo p95 (µs), three flows | split | continued after churn |
|---|---|---|---|---|---|
| baseline | 6,373,376 / 12,664,832 / 12,664,832 | 5,452,511 / 4,669,405 / 3,819,792 | 5,493,937 / 5,452,523 / 5,452,526 | 1.000 | yes |
| `balanced/1` | 2,965,504 / 6,766,592 / 6,766,592 | 255,771 / 255,769 / 251,625 | 394,724 / 394,725 / 390,799 | 1.000 | yes |
| `gaming/1` | 2,949,120 / 6,619,136 / 6,619,136 | 264,038 / 264,036 / 257,449 | 394,474 / 394,472 / 394,472 | 1.000 | yes |

The bulk columns are indexed: position 0 is the flow closed at the churn point, positions 1 and 2
are the survivors, and `delivery_min_over_max` and `continued_after_churn` use those indices. The
echo columns are **not** indexed — the harness collects each flow's ping task in completion order
and prints no flow id, so those three figures are the three echo streams' percentiles and cannot be
attributed to a named survivor: every one of the three kept completing, and they are not used to
compare arms.

All arms kept both survivors moving, delivered the closed flow's bytes up to the close, split the
survivors evenly and reported no stall; the bulk indices are what that statement rests on. No
starvation and no fairness advantage over Balanced is shown.

## 5. CPU (`relay_cpu_probe`, unshaped loopback, 128 MiB per repeat)

Whole-process user+system CPU per delivered MiB, an upper bound on the relay's own cost because the
harness, both tunnel ends and the test app share the process:

| Arm | repeat 1 | repeat 2 | repeat 3 | median |
|---|---|---|---|---|
| baseline | 3,359.4 | 5,312.5 | 5,234.4 | 5,234.4 |
| `balanced/1` | 7,421.9 | 5,000.0 | 7,265.6 | 7,265.6 |
| `gaming/1` | 12,109.4 | 11,328.1 | 14,140.6 | 12,109.4 |

`gaming/1` costs about 1.7× `balanced/1` and 2.3× Baseline per MB at the median; every `gaming/1`
repeat is above every `balanced/1` repeat. That is the price of a 4 KiB quantum and a 16 KiB cap —
four times as many wake-ups per MB — and it is a measured cost, not a benefit. The absolute windows
are short (0.43-1.81 s of CPU), so the per-MB figures are approximate; the ordering is not.

## 6. Acceptance criteria, one by one

Against the list in [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md) §6, for
`gaming/1` versus Balanced and Baseline:

| # | Criterion | State here | Label |
|---|---|---|---|
| 1 | Latency p50/p95/p99 and jitter, idle and under bulk load | Bulk-competing percentiles measured in the harness (sections 2 and 4); **idle and real-path latency not measured** | `simulated` (partial) |
| 2 | Packet loss, or a loss proxy labelled as such | Not modelled: the shaper applies serialization rate only, no loss | **not measured** |
| 3 | Throughput / goodput | Bulk goodput measured in the shaped scenario (section 3) | `simulated` |
| 4 | CPU and memory overhead | CPU per MB measured (section 5); **RSS not measured** — one process hosts every arm, so per-arm RSS is not separable | `simulated` (CPU only) |
| 5 | Fairness, no starvation | Three-flow share with a mid-run close (section 4) | `simulated` |
| 6 | Congestion / impairment (loss, jitter, path change, carrier kill) | Not modelled | **not measured** |
| 7 | Long run (60 minutes per policy) | Longest run 8 s per arm, 143 s for the whole test | **not measured** |
| 8 | Results published with an evidence label | This file; every number above is `simulated` | done for this slice |

## 7. Per-tuple claim (ADR-0047)

The claim belongs to `(engine, carrier, profile)`, and no engine inherits another's.

| Engine | Carrier | Profile | Claim | Why |
|---|---|---|---|---|
| `tcp-tls` | TCP/Yamux | `gaming/1` | **`unknown`** | No run of this tuple. The harness runs the relay and Yamux over loopback TCP with no TLS handshake; its numbers are not this tuple's. Needs the two-host LAB window |
| `noise-tcp` | TCP/Yamux | `gaming/1` | **`unknown`** | Same: no engine handshake, no real path |
| `baft` | TCP | `gaming/1` | **`unsupported`** | Out of scope by ADR-0031 decision 17 |
| any | UDP/datagram | `gaming/1` | **`unsupported`** (no carrier) | The datagram carrier is #244, not implemented. Any claim that depends on avoiding TCP head-of-line blocking needs it first (decision 11) |

The `simulated` harness numbers in sections 2-5 are a reference point for the future tuple runs, not
a claim for any of them.

## 8. What is still missing, precisely

The item's acceptance needs a real two-host LAB run — the suspended `baft-tunnel-lab` window —
covering, per `(engine, carrier, profile)` tuple: idle and bulked p50/p95/p99 latency and jitter, a
measured loss figure (or an explicit loss proxy), throughput, CPU/RSS, fairness under competing
bulk, impairment scenarios (loss, jitter, path change, carrier kill), and a run up to the
60-minute project cap. Until that window is authorized, TP-3.2 stays open, no tuple becomes
`measured`, and nothing here may be quoted as a Gaming benefit.

## 9. Reproduce

```
# on harness revision 233e440 (or any later commit that keeps this test)
POLICY_REPEATS=3 cargo test --release --features policy-pack -- \
  --ignored --nocapture gaming_profile_evidence
```

Each `POLICY_EXPERIMENT …` line is one arm run, JSON after the first object, all labelled
`simulated`. `POLICY_RATE`, `POLICY_SECONDS` and `POLICY_BYTES` change the rate, the per-arm
duration and the CPU probe size; the defaults are the figures above.

## Appendix A. Raw run output, unedited

The 24 `POLICY_EXPERIMENT` lines run `tp-3.2-local-2026-11-26` printed, followed by the test summary.
`Mechanism::label()` prints the resolved parameters, so `sched-w262144-q16384-cap65536` is
`balanced/1`'s arm and `sched-w262144-q4096-cap16384` is `gaming/1`'s; `baseline` is the pack off.

```
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"bulk_bytes_per_s":2631680} {"scenario":"gaming_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":4,"rtt_us":{"p50":2090030,"p95":2147812,"p99":2147812,"max":2147812},"bulk_bytes":21053440,"cpu_us":1130000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":2,"bulk_bytes_per_s":2631680} {"scenario":"gaming_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":4,"rtt_us":{"p50":2080693,"p95":2136313,"p99":2136313,"max":2136313},"bulk_bytes":21053440,"cpu_us":980000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":3,"bulk_bytes_per_s":2631680} {"scenario":"gaming_bulk_competing","mechanism":"baseline","evidence":"simulated","rate_bytes_per_s":2500000,"samples":4,"rtt_us":{"p50":2101010,"p95":2136034,"p99":2136034,"max":2136034},"bulk_bytes":21053440,"cpu_us":1000000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"bulk_bytes_per_s":2123776} {"scenario":"gaming_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":111982,"p95":118325,"p99":123807,"max":123807},"bulk_bytes":16990208,"cpu_us":1190000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":2,"bulk_bytes_per_s":2123776} {"scenario":"gaming_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":112642,"p95":120149,"p99":124870,"max":124870},"bulk_bytes":16990208,"cpu_us":1430000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":3,"bulk_bytes_per_s":2123776} {"scenario":"gaming_bulk_competing","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":111575,"p95":118709,"p99":122755,"max":122755},"bulk_bytes":16990208,"cpu_us":1440000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"bulk_bytes_per_s":2154496} {"scenario":"gaming_bulk_competing","mechanism":"sched-w262144-q4096-cap16384","evidence":"simulated","rate_bytes_per_s":2500000,"samples":61,"rtt_us":{"p50":110934,"p95":117617,"p99":119438,"max":119438},"bulk_bytes":17235968,"cpu_us":1170000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":2,"bulk_bytes_per_s":2121728} {"scenario":"gaming_bulk_competing","mechanism":"sched-w262144-q4096-cap16384","evidence":"simulated","rate_bytes_per_s":2500000,"samples":60,"rtt_us":{"p50":112113,"p95":118788,"p99":128400,"max":128400},"bulk_bytes":16973824,"cpu_us":1160000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":3,"bulk_bytes_per_s":2187264} {"scenario":"gaming_bulk_competing","mechanism":"sched-w262144-q4096-cap16384","evidence":"simulated","rate_bytes_per_s":2500000,"samples":62,"rtt_us":{"p50":108700,"p95":116492,"p99":117837,"max":117837},"bulk_bytes":17498112,"cpu_us":960000,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"flows":2} {"scenario":"gaming_multi_flow","mechanism":"baseline","evidence":"simulated","flows":2,"bulk_bytes":[6373376,16859136],"total_bulk_bytes":23232512,"delivery_min_over_max":1.000,"flow_p50_us":[2067892,2067890],"flow_p95_us":[3257078,3100173],"cpu_us":1190000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"flows":2} {"scenario":"gaming_multi_flow","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","flows":2,"bulk_bytes":[4276224,12009472],"total_bulk_bytes":16285696,"delivery_min_over_max":1.000,"flow_p50_us":[113184,113693],"flow_p95_us":[248333,247456],"cpu_us":1280000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"flows":2} {"scenario":"gaming_multi_flow","mechanism":"sched-w262144-q4096-cap16384","evidence":"simulated","flows":2,"bulk_bytes":[4259840,11862016],"total_bulk_bytes":16121856,"delivery_min_over_max":1.000,"flow_p50_us":[113991,113989],"flow_p95_us":[249710,249709],"cpu_us":1160000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"flows":3} {"scenario":"gaming_multi_flow","mechanism":"baseline","evidence":"simulated","flows":3,"bulk_bytes":[6373376,12664832,12664832],"total_bulk_bytes":31703040,"delivery_min_over_max":1.000,"flow_p50_us":[5452511,4669405,3819792],"flow_p95_us":[5493937,5452523,5452526],"cpu_us":1360000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"flows":3} {"scenario":"gaming_multi_flow","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","flows":3,"bulk_bytes":[2965504,6766592,6766592],"total_bulk_bytes":16498688,"delivery_min_over_max":1.000,"flow_p50_us":[255771,255769,251625],"flow_p95_us":[394724,394725,390799],"cpu_us":1280000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","repeat":1,"flows":3} {"scenario":"gaming_multi_flow","mechanism":"sched-w262144-q4096-cap16384","evidence":"simulated","flows":3,"bulk_bytes":[2949120,6619136,6619136],"total_bulk_bytes":16187392,"delivery_min_over_max":1.000,"flow_p50_us":[264038,264036,257449],"flow_p95_us":[394474,394472,394472],"cpu_us":1510000,"churn_closed":0,"continued_after_churn":true,"stalled":false}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":430000,"cpu_us_per_mb":3359.4}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":680000,"cpu_us_per_mb":5312.5}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"baseline","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":670000,"cpu_us_per_mb":5234.4}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":950000,"cpu_us_per_mb":7421.9}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":640000,"cpu_us_per_mb":5000.0}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q16384-cap65536","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":930000,"cpu_us_per_mb":7265.6}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q4096-cap16384","evidence":"simulated","repeat":1,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":1550000,"cpu_us_per_mb":12109.4}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q4096-cap16384","evidence":"simulated","repeat":2,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":1450000,"cpu_us_per_mb":11328.1}
POLICY_EXPERIMENT {"profile_run":"tp-3.2","scenario":"relay_cpu_probe","mechanism":"sched-w262144-q4096-cap16384","evidence":"simulated","repeat":3,"bytes":134217728,"delivered_bytes":134217728,"cpu_us":1810000,"cpu_us_per_mb":14140.6}
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 479 filtered out; finished in 143.49s
```

## LAB measurement of this profile on the real path (2026-11-28)

Run [46150559605](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/46150559605) on `main` `d6dcf2c` is `success`. `lab/real-hosts/policy-lab-stage.sh` drove
the two LAB hosts for six arms under `.github/workflows/policy-lab.yml`, which builds the binary
from the dispatched `main` commit with `--features policy-pack`. This profile's arm was:

```
POLICY_LAB_ARM arm=gaming-1 profile=gaming/1 pack=sched pack_engaged=1 engine=tcp-tls carrier=tcp
  bytes=134217728 delivered_bytes=134217728 sink_MiB_per_s=1.80 interactive_p50_us=109,319 interactive_p95_us=142,202
  probe_samples=419 probe_bulk_window=True sha_match=1 evidence=LAB
  policy pack engaged: profile=gaming/1 window_bytes=262144 quantum_bytes=4096 max_queued_bytes=16384 adaptive=off
```

What that is: 128 MiB of a deterministic stream that the sink, behind the tunnel on the abroad
host, hashed byte-exact (`sha_match=1`); a sink-clocked throughput of **1.80 MiB/s**; and 64-byte
echo round trips from the Iran host, sampled **only inside the bulk window** (419 samples, p50
**109,319 µs**, p95 **142,202 µs**), so the two flows competed for one tunnel session. The pack's own
`policy pack engaged` line appeared on **both** hosts with the row above and `adaptive=off`; the
control arm in the same run engaged nothing. For scale in that same run: control 19.44 MiB/s and
125,836 µs p50; `balanced/1` 1.82 MiB/s and 110,634 µs.

So `(tcp-tls, TCP/Yamux, gaming/1)` is **`measured` for this path and window**, once — one run,
so the run-to-run spread is unknown. Still **unmeasured**, and the reasons the item stays open:
`(noise-tcp, TCP, gaming/1)`; the UDP/datagram carrier, which has no arm in this harness at all
(**NOT TESTED**, not approximated); loss and jitter/impairment (no shaper on this path); RSS; the
60-minute run; repeat runs for a spread; and the adaptive controller, which was off in every arm.
No benefit is claimed for this row from this run: the scheduler rows delivered markedly less bulk
goodput than the control on this path.
