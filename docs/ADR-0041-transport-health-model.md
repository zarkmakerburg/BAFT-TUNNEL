# ADR-0041: Transport health model

Status: accepted (2026-11-17). Relates to: ADR-0028 (Integrity), ADR-0039, `READINESS_CONTRACT.md`. Roadmap: Rakhsh integration P0-3.

## Decision
1. **Health is not Integrity.** Integrity (ADR-0028) is an authenticated end-to-end data probe result. Health is a passive/active quality estimate of one link. They are separate fields in the agent API and in the panel; neither is derived from the other.
2. States per link: `connecting`, `connected` (TCP/TLS up), `authenticated` (identity verified), `healthy`, `degraded`, `recovering`, `unavailable`, and `unknown` when there is no data. `connected` never implies `healthy`; `healthy` requires `authenticated` plus enough fresh samples.
3. Metrics (all optional, absent means unknown, never zero): `rtt_ms` (smoothed), `jitter_ms`, `probe_loss_ratio` (lost keepalive probes over a window, not packet loss at IP level), `throughput_up/down_bps` (from existing counters), `uptime_s`, `reconnects`, `last_handshake_ms`, `last_failure_reason`, `queue_depth`, `buffer_bytes`.
4. Scoring is a pure function of a metric snapshot and a **documented threshold table** (configurable): each rule returns a verdict plus a human reason; the state is the worst rule verdict. No opaque number.
5. Samples older than `stale_after` (default 3 keepalive intervals) turn the metric into unknown. A hysteresis rule (N consecutive good samples to leave `degraded`) avoids flapping.
6. Measurement is in-session (keepalive round trip on the control stream), no extra ports, no raw sockets, no ICMP.
7. The panel and API show `Unknown` when there is no data. No synthetic values.

## Acceptance
Unit tests for state transitions, staleness to unknown, hysteresis, threshold overrides, and that no metric defaults to zero. API field naming keeps `health` and `integrity` as sibling objects.

## Implementation status: kernel TCP feed (2026-11-17)
The unit samples the carrier socket every 5 s with Linux `TCP_INFO` (read-only, no privilege): smoothed RTT, the share of intervals with retransmissions (named `loss_basis` in the evidence, a proxy, not probe loss), and a throughput watch (delivery rate below 64 KiB/s while the sender was busy at least half the interval and at least 16 KiB were acknowledged, for two consecutive intervals) that adds a **Degraded** reason "possible throttling (heuristic)". It cannot distinguish a throttle from a genuinely slow link and never produces Unavailable. The socket is registered only after the secure handshake and identity checks succeed, sampled only while its `getpeername` still matches, and cleared when the last session ends. No sample (non-Linux, old kernel, no session) leaves quality Unknown. Not Integrity; LAB evidence still pending.
