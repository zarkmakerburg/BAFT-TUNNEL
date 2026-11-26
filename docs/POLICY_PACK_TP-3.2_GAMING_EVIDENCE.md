# TP-3.2 Gaming profile: per-tuple evidence record

**Status: harness evidence only (`simulated`); the profile is not measured and not accepted.**
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
  2026-11-26. Source: `main` at `06aa420`; the only change under test is this test-only harness
  addition, no product code differs from that commit.
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

| Arm | flow bytes (closed, survivor, survivor) | survivor p50 (µs) | survivor p95 (µs) | split | continued after churn |
|---|---|---|---|---|---|
| baseline | 6,373,376 / 12,664,832 / 12,664,832 | 4,669,405 / 3,819,792 | 5,452,523 / 5,452,526 | 1.000 | yes |
| `balanced/1` | 2,965,504 / 6,766,592 / 6,766,592 | 255,769 / 251,625 | 394,725 / 390,799 | 1.000 | yes |
| `gaming/1` | 2,949,120 / 6,619,136 / 6,619,136 | 264,036 / 257,449 | 394,472 / 394,472 | 1.000 | yes |

All arms kept both survivors moving, delivered the closed flow's bytes up to the close, split the
survivors evenly and reported no stall. `gaming/1`'s survivor p50 is 3.2% above `balanced/1`'s,
within the per-run spread; no starvation and no fairness advantage over Balanced is shown.

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
POLICY_REPEATS=3 cargo test --release --features policy-pack -- \
  --ignored --nocapture gaming_profile_evidence
```

Each `POLICY_EXPERIMENT …` line is one arm run, JSON after the first object, all labelled
`simulated`. `POLICY_RATE`, `POLICY_SECONDS` and `POLICY_BYTES` change the rate, the per-arm
duration and the CPU probe size; the defaults are the figures above.
