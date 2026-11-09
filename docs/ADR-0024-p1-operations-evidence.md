# ADR-0024: P1 operations evidence (diagnostics, end-to-end status, traffic counters, audit)

Status: accepted (2026-11-09, under the owner's delegated decision authority).
Builds on ADR-0022 (Apply, Verify, Receipt), ADR-0023 (agent control helper) and the P1 list in
`CROSS_PROJECT_CAPABILITY_ALIGNMENT.md` §6 (items 2 to 5).

## Context

The agent ran as a separate unprivileged process next to `baft-tunnel@NAME`. It could probe the
local listener and (since P0-2) read `session.json`, but it could not tell a TCP failure from a
certificate, identity or authorization failure, had no byte counters, no end-to-end state and an
audit log without actor or target. Rule 1 of the alignment document forbids collapsing
"host reachable", "TCP reachable", "authenticated peer", "authorized session" and "data path".

## Decision

1. **The unit publishes evidence; the agent only reads it.** In `$RUNTIME_DIRECTORY`
   (`/run/baft-tunnel/NAME`, `0700 baft-tunnel`) the unit writes, atomically:
   - `session.json` (P0-2) now also names the verified `peer_id`. The Reverse service node now
     writes it too (it did not before).
   - `counters.json` every 5 s: cumulative relayed payload bytes per direction
     (`bytes_from_peer`, `bytes_to_peer`), `streams_total`, `sessions_total`, `pid`,
     `started_unix`, `sampled_unix`, `interval_s`, `units: "bytes"`. Payload bytes only; TLS/Noise
     and multiplexing overhead is excluded and the docs say so.
   - `last_failure.json`: the stage of the latest failed connection attempt and its time. No error
     text (it can hold addresses).
   The agent accepts a file only if it is a small regular file, valid JSON, written by a live
   process and, when systemd reports the unit's main PID (`--instance NAME`), by that PID.
2. **Failure stages are typed, not parsed from text.** `session_status::Stage` is attached as
   `anyhow` context where each stage can fail: `tcp` (connect), `secure_channel` (TLS 1.3 / Noise
   handshake, certificate chain, CRL), `identity` (certificate pin and URI SAN node id),
   `authorization` (protocol handshake: node ids, tunnel id, direction).
3. **`POST /v1/diagnostics`** (token, read-only apart from its own report and audit line) returns
   separate tiers, each with a `source`: `tcp_reachability` (active connect probe when this node
   dials and has no session; never claims who answered), `secure_channel`, `identity`,
   `authorization`, `session`, `data_path`, `integrity`. A tier is `pass` only on direct evidence
   (an authorized session, or a later stage failing in the recorded attempt), `fail` only on a
   recorded failure at that stage or "unit running, no session", otherwise `unknown`.
   `data_path` passes only when payload grew in both directions within 60 s. `integrity` is always
   `unknown`: only an end-to-end echo probe (Verify/LAB) measures it, and the agent never reads
   payload. The report (`verified`, `verified_unix`, `highest_passed_tier`) is stored as
   `diagnostics.json` in the state dir; `GET /v1/diagnostics` returns the last one.
4. **`GET /v1/status`** gains `tunnel`: per-tier evidence (unit, listener, authorized session,
   data path, integrity), the latest failure stage, `last_verified_unix` (from the last diagnostics
   run that reached the data path) and a summary `state`: `carrying_traffic`,
   `session_authorized`, `running_no_session`, `down`, or `unknown` when the agent has no unit
   evidence. It never says `ready`.
5. **`GET /v1/traffic`** returns the counters with units, sampling interval, source, `age_s`,
   `fresh` (≤ 3 intervals), and rates computed between consecutive samples of the same process; a
   restart or counter reset starts a new series instead of a bogus rate. Without live counters it
   returns `available: false` and Unknown.
6. **Audit v2.** Every line carries `actor` (`api_token` + a 64-bit domain-separated SHA-256
   fingerprint of the token, never the token), `target` (node id and systemd instance),
   `resource` (`desired_state`, `setup/{init,join,start}`, `diagnostics`), result, request hash,
   short detail and the durable Apply `receipt_id`. Lines are `fsync`ed before the response.
7. `baft-tunnel agent --instance NAME` binds the agent to one unit; without it unit evidence is
   reported as Unknown.

## Consequences and limits

- An active TCP probe from a dialing node can appear on the peer as an incomplete
  `secure_channel` attempt; it is only sent when no session exists.
- `identity` for `noise-tcp` is proven together with the secure channel (the pinned static key is
  the identity); a Noise failure is reported at `secure_channel`, `identity` stays Unknown.
- "Durable remote Job receipts" (alignment §6 item 4) need the remote control plane of P1 item 1
  (enrollment ADR); this ADR covers local Apply receipts only.
- Counters are in memory per process and restart at 0 with the unit; history is the agent's
  in-memory window (one hour). No long-term storage yet.

## Evidence required to call it done

Unit tests (tiers, rates, staleness, PID checks, failure staging, audit fields) and the real
systemd 249 CI step in `scripts/pki/test-agent-control-lifecycle.sh` (§4b): two units on one host,
64 KiB + 2 MiB through the tunnel, then traffic counters ≥ the bytes sent each way, diagnostics
tiers all `pass` up to `data_path` with `integrity: unknown`, status `carrying_traffic` with the
peer id; after the peer stops, `running_no_session`, session `fail`, last verified time kept;
audit lines with actor/target/resource and no token. Two-host LAB confirmation is a separate run.
