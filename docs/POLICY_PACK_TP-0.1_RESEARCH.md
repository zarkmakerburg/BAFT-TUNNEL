# Policy Pack TP-0.1: hook points, TCP/UDP and OS limits

**Status: research, 2026-11-17. Source read at `main` 5d313a9. No measurement yet; every "effect" statement below is a hypothesis for TP-1.2 to confirm or reject.** Plan: [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md). Decision: [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md). Issue [#227](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/227).

## 1. Today's data path (verified in source)

```
user socket -> pump (16 KiB buffer) -> Yamux stream -> Yamux session -> SecureStream (engine) -> TCP socket
```

| Layer | Where | Fact |
|---|---|---|
| Relay | `tunnel.rs` `pump_with_idle_timeout` / `relay_with_idle_timeout` | One task per direction per stream; fixed 16 KiB buffer; read then `write_all`; idle timeout 900 s; byte counters. No queue, no priority, no pacing. |
| Multiplexing | `tunnel.rs` `yamux_config` (`tokio-yamux` 0.3) | Per-stream window up to 4 MiB (`MAX_STREAM_WINDOW_SIZE`), keepalive 5 s, write timeout 10 s, `max_stream_count`. Stream scheduling inside the session is the library's. |
| Engine | `engine.rs` `StreamEngine` | Only secures the socket; returns `Box<dyn SecureStream>` (`AsyncRead + AsyncWrite`). `tcp-tls` = rustls; `noise-tcp` = `noise.rs` with 16 KiB plaintext records and a 64 KiB bridge. |
| Socket | `engine.rs` `tune_tunnel_socket` | `TCP_NODELAY`, keepalive, `TCP_USER_TIMEOUT` 20 s. No `SO_SNDBUF`/`SO_RCVBUF` setting, no `TCP_NOTSENT_LOWAT`. |

Consequences:

- The tunnel is **one TCP connection** carrying all Yamux streams. Loss on that connection delays every stream (head-of-line blocking). The pack cannot remove this; it can only decide what is queued ahead of it.
- Between the relay and the engine there is **no BAFT-TUNNEL-owned queue**. The Yamux per-stream window (up to 4 MiB) plus kernel socket buffers hold the bytes. A bulk stream can therefore build a queue that an interactive stream waits behind.

## 2. Candidate hook points

| # | Hook | What it can do | Cost / risk |
|---|---|---|---|
| H1 | **Relay layer**: replace the fixed pump with a policy-aware pump (per-stream write budget, pacing, priority class, bounded wait) | Admission and pacing before bytes reach Yamux; backpressure to the user socket; per-stream byte/queue metrics | Low risk, same crate, testable in memory. Cannot reorder bytes already inside Yamux or the kernel. |
| H2 | **Yamux config**: window size per class (smaller window for Bulk bounds queue depth) | Bounds how much a bulk stream can have in flight | Window is per session config today, not per stream; needs library support or per-class sessions. Not verified. |
| H3 | **Engine wrapper** (`SecureStream` decorator): a shaping writer below Yamux | Sees the merged byte stream; can pace the whole connection | Cannot distinguish streams (already multiplexed). Only whole-connection pacing. |
| H4 | **Socket options** (`SO_SNDBUF`, `TCP_NOTSENT_LOWAT`) | Shrinks the kernel queue that bulk data builds behind | Unprivileged for the socket's own options. Effective value is clamped by `net.core.wmem_max`; `TCP_NOTSENT_LOWAT` Linux-only; not a sysctl change, so it fits ADR-0030. Effect is a hypothesis. |
| H5 | **Several tunnel TCP connections** (one per class) | Real isolation between classes: a stalled bulk connection no longer blocks interactive | Largest change (sessions, authorization, reconnect, LAB). Out of Phase 2 unless H1 to H4 prove insufficient. Needs its own ADR. |

Recommendation for Phase 1-2: build the measurement harness first, then H1 (+H4 as an experiment) behind the engine-agnostic interface (#245). H2, H3, H5 stay candidates until measurement shows the gap.

## 3. TCP versus UDP

- Today only TCP carriers exist (`tcp-tls`, `noise-tcp`, BAFT engine over TCP). Hypothesis: with a single TCP connection, Gaming/Trading gains are limited to queueing delay inside BAFT-TUNNEL's control, not retransmission delay. ADR-0031 decision 11 already gates those claims on #244 (UDP/datagram carrier, separate ADR, audit and LAB).
- The policy interface must not assume a byte stream: it has to accept a future datagram path (decision 12). Section 5 sets the interface boundary accordingly.

## 4. OS limits without privileges (ADR-0030 constraint)

| Item | Unprivileged? | Note |
|---|---|---|
| Per-socket `SO_SNDBUF` / `SO_RCVBUF` | Yes (kernel doubles and clamps to `wmem_max`/`rmem_max`) | Setting disables auto-tuning for that socket; effect on throughput must be measured. |
| `TCP_NOTSENT_LOWAT` | Yes (Linux) | Limits unsent data in the socket buffer. |
| `TCP_NODELAY`, keepalive, `TCP_USER_TIMEOUT` | Yes | Already used. |
| `tc`/qdisc, `iptables`, sysctl | No (`CAP_NET_ADMIN`/root) | Forbidden by ADR-0030 and the roadmap. |
| Congestion control choice (`TCP_CONGESTION`) | Only allowed algorithms (`tcp_allowed_congestion_control`), often just the default | Not planned; not a claim. |
| Impairment for benchmarks (loss/delay/jitter) | No, on LAB hosts | Open question (plan section 4). See section 6. |

## 5. Proposed interface boundary (input to #245)

The policy engine sees **flows and units of data**, not engines. A flow declares a class (from explicit configuration, ADR-0031 decision 6); the engine reports capabilities (priority enforcement, write pacing, RTT/queue observation, datagram support). Missing capability reports Unsupported/Unknown. Proposed in [ADR-0044](ADR-0044-engine-agnostic-policy-interface.md).

## 6. Impairment method without `CAP_NET_ADMIN` (open question for TP-1.2)

1. **In-process shaper** (no privilege, deterministic): a rate-limited, delaying pipe between the two ends of an in-memory Yamux session. Simulated; never reported as LAB.
2. **Rootless network namespace in CI** (`unshare -rn` plus `tc`): needs user namespaces on the runner; not available on LAB hosts. Simulated/VM level.
3. **Real two-host LAB**: no impairment; measures the real path only, at several times of day. Subject to the LAB rules (one run at a time, isolated, cleaned up).

Phase 1 uses method 1 first. Evidence label for every result: `simulated`, `VM`, `LAB`.

## 7. What this research does not establish

- No latency, jitter or throughput number. The harness (TP-1.2) produces the first.
- Whether `tokio-yamux` shares the connection fairly between streams under load is not verified here; the harness' loaded-versus-idle scenario will show it.
- Per-engine behaviour of `noise-tcp` versus `tcp-tls` under the same load is not compared.

## 8. Harness stall: resolved (2026-11-17)

The first harness wrote to the Yamux stream directly and stalled at the 256 KiB window. Rebuilt on the product's own relay path (`relay_with_idle_timeout` on both ends, `yamux_config`), the bulk stream flows (about 29 MB in 10.5 s at a 2.5 MB/s bottleneck). The stall was a defect of the first harness, not a product defect. Why the direct-write variant stalled is not investigated further.

## 9. First Baseline numbers (evidence level: simulated, not VM, not LAB)

`src/policy_baseline.rs`, run `cargo test --release policy_baseline -- --ignored --nocapture`. Loopback TCP through a rate-limited proxy (2.5 MB/s each way, serialization delay only, no propagation delay, loss or jitter); one 64-byte ping stream (every 20 ms) with and without one competing bulk stream; pack off (today's path). One run on a development container, 10 s per scenario.

| Scenario | Samples | RTT p50 | p95 | p99 | max |
|---|---|---|---|---|---|
| idle | 424 | 2.3 ms | 3.3 ms | 3.5 ms | 35 ms |
| bulk competing | 6 | 1,970 ms | 2,004 ms | 2,004 ms | 2,004 ms |

Reading: with a bulk stream on the same tunnel, interactive round trips grew by about three orders of magnitude, consistent with the hypothesis that a deep BAFT-TUNNEL-side queue (the 4 MiB Yamux stream window plus buffers) sits in front of the bottleneck. This supports building H1/H4 (bounded queues, smaller effective window for Bulk) and is the number TP-2.1 must beat. Limits: a single run, a simulated bottleneck, no propagation delay; absolute values are not path latency and make no claim about real networks. TP-1.2 still needs repeated runs, other rates, the `noise-tcp` engine, and CI/VM and LAB stages.

### 9.1 Repeat run and aged-session scenario (2026-11-17, evidence level: simulated)

A second run of the same scenarios gave idle p50 2.3 ms and bulk-competing p50 1,949 ms (first run 1,970 ms), so the order of magnitude is repeatable on this machine; still not a statistical result.

New scenario `aged_session_throttled`: the bottleneck drops to a low rate once a connection is older than a threshold (scaled down to 3 s here; the LAB finding, run 44056840174, was flows older than about 60 s at about 21 KB/s on the real Iran-to-abroad path). A 1 MiB transfer on a fresh connection took 535 ms; on a connection older than the threshold (bottleneck 200 KB/s) it took 5,334 ms. This reproduces the shape of the LAB symptom (same bytes, an order of magnitude slower on an aged connection) and gives the stall thread's session-rotation mitigation a cheap CI-level before/after test. It models the throttle, it does not prove the LAB mechanism, and it says nothing about the real path.

### 9.2 Experiment: Yamux window cap (ADR-0048, 2026-11-17, evidence level: simulated)

`window_cap_experiment` (ignored by default) reruns the bulk-competing scenario with the per-stream maximum window overridden (2.5 MB/s bottleneck, 8 s, one run per value):

| Max stream window | Loaded ping p50 | p95 | Window / rate |
|---|---|---|---|
| 256 KiB | 98 ms | 131 ms | 105 ms |
| 1 MiB | 476 ms | 508 ms | 419 ms |
| 4 MiB (product) | 1,972 ms | 1,996 ms | 1,678 ms |

Loaded delay is close to window divided by bottleneck rate, so the queue behind the bottleneck is set by the Yamux window (plus small pipe buffers). Capping the window is a large lever (about 20x here) but `tokio-yamux` requires `max_stream_window_size >= 256 KiB` (initial window), so a window cap alone bottoms out near 100 ms at this rate: still about 40x the idle 2.3 ms and short of the ADR-0048 hypothesis (p95 within 10x of idle). Getting lower needs a per-class budget below the stream window (M1) or a smaller socket buffer (M3). Caveats: single run per value; simulated bottleneck without propagation delay; a smaller window also lowers the bandwidth-delay product a stream can fill on a long path, so a window cap can reduce throughput on real high-RTT links and must be tested with a delay model before it is considered.

### 9.3 Experiment: ADR-0048 mechanism matrix (2026-11-26, evidence level: simulated)

`mechanism_experiments` (ignored by default) runs the same bulk-competing scenario as 9.2 with each in-flight mechanism applied to the bulk flow, at three bottleneck rates, two repeats, 6 s per run. Run command: `POLICY_RATE=<rate> POLICY_REPEATS=2 POLICY_SECONDS=6 cargo test --release --features policy-pack policy_baseline::tests::mechanism_experiments -- --ignored --nocapture`. Loaded ping p50 in milliseconds (repeat medians):

| Arm | 1 MB/s | 2.5 MB/s | 5 MB/s |
|---|---|---|---|
| Idle (no bulk) | 4.5 | 4.4 | 4.4 |
| Baseline (product 4 MiB window) | 4,193 | 2,026 | 1,186 |
| M1 credit budget, 2 MiB | 2,246 | 977 | 590 |
| M2 receive pacing, 64 KiB | 4,178 | 2,017 | 1,263 |
| M3 socket backpressure, 32 KiB / 16 KiB | 4,162 | 2,094 | 1,325 |
| Window 256 KiB | 271 | 110 | 61 |
| Scheduler on the 256 KiB window (`sched`, section 9.4) | 270 | 109 | 62 |

At 2.5 MB/s the window arm's p95 is 119 ms (idle p95 5.2 ms), the scheduler's 118 ms, the M1 arm's 1,019 ms, Baseline's 2,088 ms.

What the run establishes, all `simulated`:

- **M1 cannot bound below the window.** Yamux's `StreamHandle::send_window_update` sends an update only when `delta >= max_recv_window / 2`, and the receiving side grants `max_recv_window - 256 KiB` once at stream creation. With the product's 4 MiB window a 64 KiB credit budget therefore stops at 64 KiB and never resumes: `credit_budget_below_the_window_granularity_stalls` (a 256 KiB transfer does not finish). At exactly 2 MiB the flow completes (`credit_budget_at_half_the_window_completes`) and the bound is 2 MiB — the 1,045 ms row.
- **M2 and M3 do not move the queue.** Both measured within run-to-run variance of Baseline at all three rates: the bytes are already in the sender's Yamux window before either the socket buffer or the receiving sink matters.
- **The window is the lever.** 256 KiB cut p50 by 18x at 2.5 MB/s and 21x at 5 MB/s. The rerun of the 9.2 experiment reproduces its numbers within about 15% (256 KiB 114 ms, 1 MiB 526 ms, 4 MiB 2,130 ms p50, single run).
- **The aged-session throttle is unchanged.** `aged_session_under_window_cap`: fresh 545 ms / aged 5,381 ms against Baseline 544 / 5,370 ms — the bound neither hides nor worsens it.
- **`bulk_bytes_per_s` is not goodput.** It counts bytes handed to the engine; the shaped pump fixes the delivered rate in every arm. The window arm hands over less (2.1 versus 2.8 MB/s at a 2.5 MB/s bottleneck) because it queues less, which is the point. Real-path goodput under a smaller window is not measured here.

Decision: the in-flight bound is the per-stream window, 256 KiB, applied by `tunnel::yamux_config` and switchable with `BAFT_TUNNEL_POLICY_PACK=window-cap[:<bytes>]` (`src/policy_relay.rs`). M1/M2/M3 are not wired; see [ADR-0048](ADR-0048-policy-queues-and-scheduler.md) decision 4 and its results section. The remaining gap to the p95-within-10x-idle hypothesis (met at 5 MB/s, not at 2.5 or 1 MB/s) is the queue in front of the interactive flow, which the per-class scheduler (`src/policy_sched.rs`) is meant to bound and which is still not called by the relay.
### 9.4 Experiment: scheduler wired to the relay, TP-2.2 (2026-11-26, evidence level: simulated)

The same matrix re-run with the shipped `sched` switch as a sixth arm: one deficit round-robin arbiter per relay direction, 16 KiB quantum, 64 KiB per-flow cap, on the same 256 KiB window as the `window-cap` arm. The comparison therefore isolates arbitration, not the in-flight bound. Commands are those of 9.3; the arm is `sched-w262144-q16384-cap65536`.

| Arm | 1 MB/s p50 | 2.5 MB/s p50 | 5 MB/s p50 | 2.5 MB/s p95 | 2.5 MB/s p99 | bulk handed at 2.5 MB/s |
|---|---|---|---|---|---|---|
| Idle | 4.5 | 4.4 | 4.4 | 5.2 | 6.3 | 0 |
| Window 256 KiB | 271 | 110 | 61 | 119 | 138 | 2.21 MB/s |
| `sched` on that window | 270 | 109 | 62 | 118 | 144 | 2.22 MB/s |

Findings, all `simulated`:

- **No benefit, no regression in this scenario.** Every arm-level difference is inside the repeat-to-repeat spread; p99 is 6 ms worse at 2.5 MB/s and 5 ms better at 5 MB/s. Nothing here justifies turning the switch on by default, so it stays off.
- **The reason is structural.** With one bulk flow and one interactive flow the window already bounds the queue, and DRR only decides the order of 16 KiB chunks inside it; it cannot shrink the window's worth of queued bytes. Fair scheduling between classes/flows is the case it exists for, and that is TP-3.x plus a multi-flow measurement.
- **Bounded queues verified by tests, not by numbers.** Per-flow queued bytes never exceed one quantum; an over-cap request is refused with an error that ends the flow rather than growing memory; a closed or dropped flow leaves the rotation and the arbiter reports zero flows after the relay ends (`policy_sched` and `policy_relay` tests).
- **Idle timeouts and the aged-session throttle are unchanged.** `aged_session_under_scheduler`: fresh 538 ms / aged 5,392 ms against Baseline 545 / 5,371 and window-cap 544 / 5,404.
- **Per-MB CPU of the arbiter path is not measured.** The ADR-0048 decision 6 overhead budget therefore remains unverified; the only cost evidence is the unchanged latency and handed-bytes above.
- **No LAB evidence.** The two-host LAB window is still suspended, so every number here is loopback harness evidence labelled `simulated`.

