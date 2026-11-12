# Readiness and integrity contract

Rule: no API field, panel text, receipt or report claims more than the evidence it holds. Missing evidence is `unknown`, never `pass`. Nothing in the agent or panel says "ready".

## Five levels (each needs its own evidence)

| Level | Meaning | Evidence | Who can establish it | Where it shows |
|---|---|---|---|---|
| 1. Session authenticated | The peer completed the secure channel, certificate or Noise key pin, identity and authorization handshake. | The unit wrote `session.json` after the handshake (`authorized_session: observed`, peer id, since). | Agent (reads the unit's file; accepted only from the unit's main PID) | `GET /v1/status` `tunnel.tiers.authorized_session`; diagnostics tiers `secure_channel`, `identity`, `authorization`, `session`; state `session_authorized` |
| 2. Transport connected | A TCP connection to the peer endpoint works, or an authorized session exists. | Active connect probe when no session exists (never says who answered), or the session itself. | Agent | diagnostics `tcp_reachability`; `listener` tier. TCP reachability alone never means a working tunnel |
| 3. Data path verified | Relayed payload grew in both directions within 60 s. | Unit counters (`counters.json`): bytes to and from the peer. Excludes TLS/Noise overhead. | Agent (counters only; it never reads payload) | diagnostics `data_path`; `GET /v1/traffic`; state `carrying_traffic` |
| 4. End-to-end integrity verified | A fresh 64 KiB payload sent by the in-product probe arrives unchanged at the authenticated peer. | Protocol-v2 IntegrityProbe on the active authenticated session; explicit user action; 60-second freshness window. | Tunnel unit responder, initiated by the Agent over the per-instance UDS | In-product result is Pass only while fresh; history remains visible after expiry. LAB 64 KiB/2 MiB evidence remains separately recorded |
| 5. Operational receipt ready | An Apply receipt may say the node is ready for production use. | Would need level 4 for this apply, plus owner-defined release criteria. Not implemented. | Nobody yet | Apply receipt `ready: "unknown"` always; `session` is `observed` or `unknown` |

Levels are not cumulative claims: a higher level never follows from a lower one. `session_authorized` does not imply `carrying_traffic`; `carrying_traffic` does not imply integrity.

## API and panel rules

- `tunnel.state` is one of `carrying_traffic`, `session_authorized`, `running_no_session`, `down`, `unknown`. Never `ready`.
- Diagnostics highest_passed_tier may reach integrity only after a successful fresh in-product probe. The verified field describes observed data-path verification; it is not production readiness. Apply receipt ready remains Unknown.
- Without unit evidence (no instance, unreadable runtime files, wrong PID) the unit and session tiers are `unknown`. `listener_accepting` is a separate, weaker fact.
- Counters older than three sampling intervals are `fresh: false`; no rate is invented across a process restart.
- Apply receipt `ready` is `"unknown"` in every outcome. `session: observed` means the unit recorded an authorized session after start, not that bytes flow. The LAB stages assert a receipt claiming ready fails the run.
- The panel renders only values the API returned. A failed or missing endpoint shows Unknown or Unavailable.

## Verification status (what has actually been run)

- Unit tests cover Unknown without a recent probe and expiry back to Unknown after 60 seconds; Apply receipt ready remains Unknown.
- Mocked-API browser run: `scripts/panel-qa/p1-ops.js`.
- Real agent and real tunnel, local, no LAB hosts: `scripts/panel-qa/p1-real-agent.js` against `baft-tunnel agent` with a real noise-tcp serve and forward pair and a real echo service. This is not a LAB or two-host result.
- Real two-host LAB: owned by the Reverse and Direct Operations acceptance stages (see PHASE1_IMPLEMENTATION_STATUS.md for run URLs). Not claimed here.
