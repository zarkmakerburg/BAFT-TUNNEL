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
