# ADR-0039: Carrier link policy (failover, failback, backoff)

Status: accepted (2026-11-17). Relates to: ADR-0010, ADR-0018, ADR-0021, ADR-0038. Roadmap: Rakhsh integration P0-1.

## Context
A tunnel has one link (one address, one engine). When it fails the agent redials the same link with a 1 s to 60 s doubling backoff (`src/tunnel.rs`). There is no way to prefer a second path, and no memory of which path failed.

## Decision
1. A **link** is an ordered, explicitly authorized endpoint of one tunnel: `(engine, address)`. Today the only engines are `tcp-tls` and `noise-tcp`; "carrier" in this ADR means link. New carrier types (QUIC, WS) join later through the same interface, each with its own ADR.
2. A pure, I/O-free **LinkPolicy** state machine decides what to dial. The tunnel loop supplies events (`connect_started`, `connect_failed`, `session_established`, `session_lost`, tick) and the current time, and receives the next link and delay. Engines and transports stay unaware of it.
3. Selection: highest-priority link that is not in penalty; if all are penalized, the one whose penalty ends first. Deterministic and explainable (the decision carries a reason string).
4. Penalty: per-link exponential backoff (initial 1 s, factor 2, cap 60 s) with deterministic bounded jitter supplied by the caller (no hidden randomness in the policy), reset only after a session on that link stayed up for `stable_after` (default 30 s). A session that dies sooner than `stable_after` counts as a failure, so a link that handshakes and is then cut does not look healthy.
5. Anti retry-storm: at most one dial attempt in flight per tunnel; total dial rate is bounded by the minimum of all penalties; link count per tunnel is capped (default 4).
6. Failback: while running on a non-primary link, a better link may be probed no more often than `probe_interval` (default 60 s, minimum 15 s) and only if it is out of penalty. The move happens only after a full authenticated handshake on the better link, and only after the better link has been healthy for `failback_hold` (default 60 s) to prevent flapping. Without stream resume (ADR-0040) a failback closes the current session, so failback is **off by default** until resume exists, on by explicit config.
7. Identity and authorization are per tunnel, not per link: every link must present the same node identity; a link to a different identity is rejected.
8. No firewall, route or sysctl change. No new privileges.

## Acceptance
Unit tests: ordering, backoff growth and cap, reset only after stability, short-lived-session penalty, no flapping under alternating success/failure, one in flight, link cap, identity mismatch rejection hook. Integration (local, simulated) tests drive the tunnel loop with a fake clock. Evidence level: CI/local only until a real two-host LAB run.

## Rollback
Config with a single link behaves as today. Feature is additive; removing `link` entries restores current behaviour.

## Implementation status (2026-11-17)
Wired: `backup_remotes` (JSON config, max 3, same peer identity, not for the baft engine) feeds `LinkSet` in `src/tunnel.rs`; failover and per-link backoff apply to the dialing forward loop and the reverse service loop. Single-link configs keep the original loop unchanged. Not done: failback (policy supports it, kept off), link status in the agent API, panel view, setup/desired-state support for backups, LAB evidence. Failure kinds are all `Network` for now (identity failures fail TLS/Noise and are penalized like network failures).

### Setup support (2026-11-17)
`setup plan`/`init` accept `--backup-peer-host IP` (repeatable, max 3, forward role only, not the baft engine). Each becomes a `backup_remotes` entry on the tunnel port; hosts must be distinct IPs and differ from the primary. The list is persisted in the setup state so `join` keeps it, travels through the agent plan body and the helper request, and is part of the request digest. All carriers share the same tunnel identity and credentials (no per-link trust). Not LAB-tested.
