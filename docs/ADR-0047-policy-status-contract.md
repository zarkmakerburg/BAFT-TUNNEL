# ADR-0047: Policy Pack status contract (API, CLI, panel)

**Status:** Accepted 2026-11-27 under the owner's delegated decision authority. The read surface and
its panel card are **implemented and merged** (2026-11-27, § "Implementation plan"): the tunnel
publishes `policy.json`, `GET /v1/status` carries the `policy` member, `baft-tunnel policy status`
renders it, and `panel-react`'s `PolicyCard` shows the same object in fa/en. The write surface it sits beside shipped earlier
([ADR-0057](ADR-0057-policy-operator-surface.md), merged 2026-11-27).
**Date:** 2026-11-17, accepted 2026-11-27 after reconciling it with the shipped write surface and the
pack's `simulated` records.
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decisions 2, 8, 10, 14 and 15-19,
[ADR-0044](ADR-0044-engine-agnostic-policy-interface.md), [ADR-0024](ADR-0024-p1-operations-evidence.md)
(Unknown, freshness, published evidence), [ADR-0022](ADR-0022-apply-verify-receipt.md),
[ADR-0055](ADR-0055-tunnel-remove-and-edit.md), [ADR-0056](ADR-0056-adaptive-optimization-controller.md),
[ADR-0057](ADR-0057-policy-operator-surface.md), issues
[#240](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/240) and
[#241](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/241).

## Context

TP-5.1 (API/CLI) and TP-5.2 (panel) must never show a profile as working without evidence, and never
fill a gap with a made-up number (project rule: no fake data in the panel). This ADR fixes the shape
of the status so engine work and UI work can proceed independently.

The first draft (2026-11-17) was written when the pack had no operator surface at all. Three things
have changed since, and this acceptance is reconciled with them rather than with the draft's
assumptions:

1. **The write side now exists and is merged** ([ADR-0057](ADR-0057-policy-operator-surface.md),
   merged as `5e5a8aae` and its note as `8d1ecce4`): one typed `POST /v1/policy/apply` with
   `enable`/`disable`/`select`/`reset`, writing the managed unit drop-in
   `50-policy-pack.conf` (`Environment=BAFT_TUNNEL_POLICY_PACK=sched`) and `policy.profile` /
   `policy.adaptive` in `/etc/baft-tunnel/NAME.json` through the ADR-0055 root helper, with
   restart-verify and rollback, plus a thin loopback `baft-tunnel policy …` CLI. The receipt
   deliberately says `configured`, never "active". Status is therefore the **only** place allowed to
   answer "is it running, and with what".
2. **Six `simulated` pack records now exist and none of them is a tuple.** TP-3.2 Gaming, TP-3.3
   Trading, TP-3.4 Streaming and TP-3.5 Bulk each ship a per-tuple record at the `simulated` level
   whose own §8 records the tuples as `unknown`, because the harness runs the product relay and
   Yamux over loopback TCP with no engine handshake — its line is neither `(tcp-tls, TCP, …)` nor
   `(noise-tcp, TCP, …)`. TP-4.1's adaptive record and TP-4.2's fail-safe record are `simulated` as
   well, and TP-4.2 recorded that `violations` was a constant `0` on the scheduler path because
   nothing constructs a `Guarded<TrafficPolicy>` there. Since 2026-11-27 the arbiter's own
   `guard_violations` counter feeds that sample (tested), so the sampling gap is closed; the unit's
   published running evidence still does not carry the counter, which is why this contract keeps the
   field `available:false` rather than printing a number. A status surface that cannot say
   `unknown`, or that prints an unobserved value as a measurement, would contradict ADR-0031
   decisions 10 and 14 and every one of those records.
3. **No process currently publishes what the running tunnel resolved.** The unit publishes
   `session.json`, `counters.json` and `last_failure.json` (ADR-0024 decision 1); nothing names the
   switch or the profile a session actually used, and the agent is unprivileged (SA-09), so it
   cannot read another unit's `/proc/<MainPID>/environ`. Without a published fact, any `active`
   verdict would be a guess from files that may never have been restarted into.

## Decision

1. **One read-only `policy` member of the existing `GET /v1/status`.** It uses the same bearer
   token, the same loopback listener and the same `--instance` binding as every other status read,
   and it is additive: existing members are unchanged. There is **no `GET /v1/policy`** and no write
   path in this ADR; the CLI and the panel read this object, so the pack has one status owner instead
   of three surfaces. When the agent has no instance, `policy` is `{"state":"unknown",
   "reason":"no_instance", …}` rather than an omitted key, so a client cannot mistake absence for
   "off".

2. **Three evidence sources, each labelled, in this order of authority for the question it answers.**
   The object never merges them into one unlabelled field.

   - **Running facts — the unit's published `policy.json`.** Following ADR-0024 decision 1, the
     tunnel process writes `policy.json` atomically into `$RUNTIME_DIRECTORY`
     (`/run/baft-tunnel/NAME`, `0700 baft-tunnel`) at start and on every policy change (session
     start, a restart after `select`/`reset`, an adaptive row change), carrying
     `schema`, `switch` (the raw `BAFT_TUNNEL_POLICY_PACK` string it saw, or `null`),
     `feature_compiled`, `engine`, `carrier`, `configured_profile`, `resolved_profile`,
     `adaptive_effective`, `fallback` (`null` or `{cause, since_unix}`), the `metrics` snapshot of
     decision 8, and `pid`, `started_unix`, `sampled_unix`, `interval_s`. The agent accepts it only
     from a live `MainPID` when systemd reports one, and only while fresh (decision 11), exactly as
     it already does for `session.json` and `counters.json`. This file, and only this file, can
     produce `active`, `fallback`, `profile.running` or an effective `adaptive` value. The default
     build writes it too, with `feature_compiled:false` and no resolved profile, so "off because the
     feature is not compiled" is an observed fact rather than an inference.
   - **Configured intent — the operator's files, read-only.** `profile.configured` and
     `adaptive.configured` come from the tunnel config the agent already loads. The switch comes
     from the same read-only filesystem scan the helper performs (the instance unit, the template
     unit, `/etc/systemd/system/{NAME.service.d,baft-tunnel@NAME.service.d}`), reported per source.
     A source that cannot be read makes the configured switch `unknown`; two sources that both set
     it make it `unknown` too, with `reason:"switch_set_in_multiple_sources"`, because which
     assignment systemd applied is not the agent's to guess. No helper op and no new privilege is
     added for this: the drop-ins are world-readable and the scan is the pure filesystem part of the
     helper's existing check.
   - **Build fact — `feature_compiled` from the agent's own compile-time constant.** This is the same
     constant ADR-0057 decision 3 already gates `enable`/`select` on, not a second capability check.

   The agent never writes, never calls `systemctl` for this, never reads `/proc/<pid>/environ` and
   never asks the helper for a status: the helper's authority is the write (ADR-0055), and a read
   that needed root would put the pack's status behind a privilege the rest of `/v1/status` does not
   have.

3. **State is a closed set — `off`, `active`, `fallback`, `unknown` — with a closed `reason`.** The
   decision table, applied in order:

   | Condition | `state` | `reason` |
   |---|---|---|
   | feature not compiled in **and** the switch is conclusively not on | `off` | `feature_not_compiled` |
   | feature not compiled in, switch on in a file | `off` | `feature_not_compiled` (the pack cannot run; the write surface refuses `enable` for the same reason) |
   | feature compiled in, every readable source says the switch is absent/empty/`off` | `off` | `switch_absent` / `switch_off` |
   | fresh running evidence, pack on, `fallback` null | `active` | `null` |
   | fresh running evidence, pack on, `fallback` non-null | `fallback` | the fallback's `cause` |
   | switch on in configuration, no fresh running evidence (unit stopped, restarting, file stale or missing) | `unknown` | `no_running_evidence` / `stale_running_evidence` / `unit_not_running` |
   | configured switch cannot be established (unreadable source, two sources) | `unknown` | `config_unreadable` / `switch_set_in_multiple_sources` |
   | no agent instance | `unknown` | `no_instance` |

   `active` is **never** derived from configuration. A tunnel whose file says `gaming/1` and whose
   switch is on, but which has not been restarted since (no `policy.json` for the current
   `MainPID`), is `unknown`, not `active` — which is exactly the state the write receipt's
   `restart_required` describes.

4. **Configured and running profile are separate fields, and only one may be called running.**
   `profile.configured` (id or `null`, `source:"config"`), `profile.configured_effective`
   (`balanced/1` when absent, the identity row of ADR-0031 decision 15), `profile.running` (id or
   `null`, from running evidence only) and `profile.class` (the id the session applies to every flow,
   ADR-0031 decision 16 — `null` without evidence). The CLI text form and the panel label the first
   as "configured" and the second as "running"; no surface may print a configured id as the active
   one.

5. **Per-tuple evidence, not per-profile, and no `ready` value anywhere.** Each entry carries
   `engine`, `carrier`, `profile`, `claim` ∈ {`unsupported`, `unknown`, `measured`} and
   `evidence` (`null`, or `{level, run_id, sha, document}`). The rules:

   - `measured` requires a record **for that exact tuple** whose run actually exercised that engine
     and carrier, with `level` ∈ {`simulated`, `vm`, `lab`}, its run id and the exact SHA. `lab` is
     only a real two-host LAB run. Production is never a level.
   - A harness line produced over loopback with no engine handshake is **not** a tuple record, so it
     cannot make any tuple `measured`, whatever its numbers say. This is why the four TP-3.2-TP-3.5
     records and the TP-4.1/TP-4.2 records leave every row below `unknown`.
   - `unsupported` means the code cannot apply a profile there at all: `baft` (ADR-0031 decision 17)
     and the datagram carrier (#244; ADR-0046 accepted 2026-11-27 for its design, no carrier yet).
     Unsupported rows are shown, not hidden.
   - A tuple with no record is `unknown`; absence is never rendered as `measured` or as a zero.

   The object carries one entry per profile row (the five of decision 15) on the tunnel's own
   engine and `tcp` carrier, plus an explicit `unsupported` entry for `baft` or `datagram` when that
   is the configured engine/carrier. Results are never carried across engines or carriers. Today the
   table loads with **zero `measured`** entries, because that is the truth of the shipped records.

6. **Capabilities are shown, per ADR-0044 decision 4.** For the configured (engine, carrier) the
   object carries the five interface capabilities — `priority_enforcement`, `write_pacing`,
   `rtt_observation`, `queue_observation`, `datagram` — each `supported`, `unsupported` or `unknown`,
   defaulting to `unknown` when the declaration is unavailable (`src/policy.rs` `Capabilities`
   already defaults to `Unknown`, and a capability is never assumed). No aggregate "all supported"
   boolean is exposed as readiness; `all_supported` is not a status verdict.

7. **Metrics are ADR-0024 values, never a filled zero.** Every metric is
   `{value, unit, interval_seconds, provenance, fresh, available}`: `provenance` is the literal
   `measured` or `estimated` and measured and estimated series are separate fields, never merged;
   `fresh` uses the ADR-0024 rule (stale after three intervals); a value that was not observed is
   `null` with `available:false` and a `reason`, never `0`. Packet loss stays a proxy and is labelled
   `loss_proxy`. The block covers `queued_bytes`, `overflow_refusals`, `guard_violations`,
   `queueing_delay_estimate_ms`, `adaptive` (effective on/off, `changes`, `last_change` with
   `at`/`from`/`to`/`cause`) and `fallback` (cause, since).

   Two reconciliations with the shipped records are binding:

   - **`guard_violations` is `available:false` while the published evidence does not carry it.**
     TP-4.2 recorded a constant `0`; since 2026-11-27 the arbiter counts real guard violations and
     the adaptive sample uses that counter (tested — see ADR-0056's update note), but `policy.json`
     still omits it, so the status surface has nothing to publish. The contract forbids inventing a
     number: the field stays `null`, `available:false`, `reason:"not_instrumented"`, now meaning
     "not published to this surface", not "no counter exists".
   - **`queueing_delay_estimate_ms` is `available:false`, `reason:"signal_blind"`, while the
     sampler cannot see waiting.** TP-4.1 records that the arbiter's queue is a rendezvous: the grant
     is handed out inside the loop, `queued_bytes` samples as `0`, and the bytes that really wait are
     blocked in `write_all` behind the Yamux window. A `0 ms` estimate from that signal is "not
     observable", not "no delay", and the object says so instead of showing `0`. When a future
     sampler can see the wait, the field becomes `estimated` with its value and no other rule here
     changes.

8. **The running snapshot is small, typed and bounded.** `policy.json` carries only the fields of
   decisions 2-7 plus its own identity (`pid`, `started_unix`, `sampled_unix`, `interval_s`). It
   carries no file paths, no key material, no token, no peer addresses and no payload data, and it
   is read with the same size bound and live-writer check as the other runtime files.
   It is a status record, not an audit trail: the durable history of what was written stays in the
   write receipts and the `policy_apply` audit lines of ADR-0057.

9. **CLI: `baft-tunnel policy status --instance NAME [--agent URL] [--token-file PATH] [--json]`.**
   It reads the same `GET /v1/status` as the write verbs' client, prints the `policy` object as JSON
   or as a short text form, and, like them, refuses a non-loopback agent before sending anything and
   performs no interpretation of its own. The text form shows: state and reason; configured profile
   and — only with evidence — the running one; switch on/off/unknown with its source; adaptive
   on/off/unknown; the fallback cause if any; the metric block with `unknown` where a value is
   unavailable; and the per-tuple table with its evidence chip. **Exit codes:** `0` whenever the
   agent answered, including `state:"unknown"` (an unknown reading is a successful read of an
   unknown fact); `2` when the agent cannot be reached, is not authorised, or the instance does not
   match — the same failures the read already has, never retried as a write.

10. **Panel (TP-5.2, #241) consumes this object and adds no field of its own.** One "Traffic policy"
    card, fa/en and RTL: a state badge (`off`/`active`/`fallback`/`unknown`), the configured profile
    labelled as configured, the running profile only when present, switch and adaptive on/off/
    unknown, the last fallback cause, the per-tuple claim table with an evidence-level chip, and the
    metric values with their freshness. Copy states plainly that policy affects only BAFT-TUNNEL's
    own queueing, not internet path latency (ADR-0031 decision 4). The card is hidden when the state
    is `off` and the feature is not compiled in; with no measurements it says "no measurements yet"
    and draws no chart; unavailable values are `unknown`, never `0`; state is never encoded by colour
    alone (release-note item 5); an `interface_version` newer than the client's shows the raw state
    plus a note and interprets nothing.

11. **Freshness and compatibility.** `policy.json` is sampled at `interval_s` (1 s for the running
    facts and the adaptive counters); a value older than three intervals is stale. Stale running
    evidence never becomes `active` or `fallback`: the running fields go `null`, the metric series go
    `fresh:false` and the state falls to `unknown` with `reason:"stale_running_evidence"`. The object
    carries `interface_version` = `POLICY_INTERFACE_VERSION` (1, ADR-0044); additions are additive
    only, and a client that sees a newer version shows the raw state without interpreting claims.

12. **What the read must never do** (the enforceable half of ADR-0031 decisions 10 and 14): call a
    configured profile active; show `measured` without a tuple record; infer a result from another
    engine, carrier or profile; print `0` for an unobserved metric; hide an `unsupported` tuple;
    render `unknown` as `off` or as working; write or store a receipt from a read.

## Consequences

- Positive: the write receipt and the status finally meet — "configured" is what was written,
  "running" is what the unit published, and the distance between them (`restart_required`, stale
  evidence, `unknown`) is visible instead of implied. UI and engine work proceed against one
  contract, and no path exists to display an unmeasured claim.
- Cost: the tunnel must publish one small status file (a `policy.json` in the run directory) before
  any `active` or `fallback` can appear; until that ships, the object is honestly `unknown`/`off`.
  An evidence record store is still required before any `measured` claim can appear, and the
  two-host LAB is the only source that can fill it.
- The `simulated` records keep their label and their `unknown` tuples. Accepting this ADR changes no
  measurement, turns nothing on (the pack stays off behind `policy-pack` and
  `BAFT_TUNNEL_POLICY_PACK`), and claims no benefit.

## Implementation plan (shipped 2026-11-27)

In this order, as one implementation PR after this acceptance:

1. **Publish** `policy.json` from the tunnel process (both builds), atomically, at start and on
   every policy change, with the fields of decision 8; its absence is a normal state, not an error.
2. **Read** it in the agent as decision 2 describes, and compose the `policy` member of
   `GET /v1/status` per decisions 3-7 and 11, with the closed `reason` set.
3. **Add** `baft-tunnel policy status` per decision 9.
4. **Test**: the decision table of decision 3 case by case (including "configured on, no running
   evidence → `unknown`", "stale evidence → `unknown`", "two switch sources → `unknown`"); a
   default-build object with `feature_compiled:false` and every tuple `unknown`; a feature-build
   object with `active` only from a matching, fresh `policy.json`; `measured` unreachable from a
   `simulated` harness record; `guard_violations` and the delay estimate `available:false` with their
   reasons; no metric found as `0` when absent; CLI exit codes and non-loopback refusal; and the
   existing default-path test that the data path is unchanged with no switch and no profile.

All four steps shipped on 2026-11-27 in one scoped PR merged to `main`: `src/policy_status.rs`
carries the publisher (`start_publisher` at unit start in both `serve_versioned` and
`forward_versioned`, `note_row` on an accepted adaptive change) and the composer (`compose`, wired
into `agent::status` with a single `unit_view` read for both members); `src/policy_cli.rs` carries
`status` (bound-instance check, then `GET /v1/status`) and `render_status`, dispatched by
`PolicyCommand::Status` in `src/main.rs` with exit 0 for an `unknown` reading and 2 for an
unreachable or mismatched agent. The tests cover decision 3's table case by case, the gate-off build,
the absent/off/multi-source switch, stale and foreign-pid evidence, fallback causes, every tuple
`unknown` (and `baft` `unsupported`), null-not-zero metrics, the `policy` member in the agent's
status route, and the CLI rendering. TP-5.1 (#240) closes on that merge. TP-5.2 (#241) then built the fa/en card on that object and
merged it on 2026-11-27: `panel-react`'s `PolicyCard` renders exactly the fields above (state and
reason, configured vs running, switch with sources, adaptive, fallback, tuple claims and metrics as
unavailable-with-reason rather than 0), and its pure view model marks a gate-off build, an absent
`policy` member and a failed/401 status read unavailable with a reason instead of inventing a
healthy state; its decision table runs in the panel's `npm test`. TP-5.3 documents the pack.

## Rejected

- **A single "profile ready" flag, or any `ready` value.** Hides per-engine differences and
  contradicts ADR-0031 decision 14.
- **Deriving `active` from the config file or from the write receipt.** The file may never have been
  restarted into and the receipt says `configured` by design (ADR-0057 decision 6); only published
  running evidence can say active.
- **Reading `/proc/<MainPID>/environ` or calling `systemctl` from the agent.** The agent is
  unprivileged (SA-09) and a status read must not need root; the unit publishes the fact instead,
  as ADR-0024 already established.
- **A separate `GET /v1/policy` now.** One status owner; a second route would be a second place to
  keep the unknown rules correct. It may be considered only if a non-status consumer needs it.
- **Showing a default, estimated or zero latency when nothing was measured.** Including the constant
  `0` guard counter TP-4.2 recorded.
- **Letting a `simulated` harness record satisfy a tuple.** The harness line is not the tuple, and
  every TP-3.x record already says so.

## Not covered (stated, not faked)

- The pack's documentation page (TP-5.3) and the UDP carrier (#244). (The implementation and the
  panel card (TP-5.2) have since shipped: 2026-11-27.)
- Any measured profile: no tuple is `measured` and none can become so without a real two-host LAB
  window, which stays suspended.
- Turning the adaptive controller on: `policy.adaptive` is reported, never set, by this surface.
