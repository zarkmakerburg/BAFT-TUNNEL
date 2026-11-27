# ADR-0057: Policy Pack operator surface (Agent API and CLI)

**Status:** Accepted 2026-11-27 under the owner's delegated decision authority. Write surface only; the
status/metrics read is deferred to [ADR-0047](ADR-0047-policy-status-contract.md) (still Proposed).
**Date:** 2026-11-27
**Related:** [ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decisions 2, 10, 14 and 15-19,
[ADR-0047](ADR-0047-policy-status-contract.md), [ADR-0044](ADR-0044-engine-agnostic-policy-interface.md),
[ADR-0022](ADR-0022-apply-verify-receipt.md), [ADR-0024](ADR-0024-p1-operations-evidence.md),
[ADR-0023](ADR-0023-agent-control-helper.md), [ADR-0055](ADR-0055-tunnel-remove-and-edit.md),
[ADR-0048](ADR-0048-policy-queues-and-scheduler.md), [ADR-0056](ADR-0056-adaptive-optimization-controller.md),
issues [#240](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/240) and
[#241](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/241).

## Context

Three inputs gate the pack today, and each has a different owner:

1. the cargo feature `policy-pack` — the build;
2. the runtime switch `BAFT_TUNNEL_POLICY_PACK` (`unset`/`off`, `window-cap[:<bytes>]`,
   `sched[:<quantum>[:<cap>]]`) — the deployment, read from the unit's environment;
3. the tunnel config's `policy` object (`policy.profile`, `policy.adaptive`, `PolicySettings` in
   `src/config.rs`) — the operator's row and opt-in.

Only (3) has a schema and a validation path in the code. The switch is an environment variable an
operator sets on the unit (`systemctl edit`), and no API or CLI route changes any of the three: the
panel can create, start, stop, edit and remove a tunnel, but a profile or the pack switch is edited by
hand. TP-5.1 (#240) asks for the pack's operator surface — enable/disable, profile selection, reset to
Balanced, and a read of the active profile and metrics — in the typed, validated, idempotent and
audited pattern of ADR-0022/0024.

Two constraints shape the answer. First, ADR-0031's on/off is deliberate and layered: adding a fourth
input (for example `policy.enabled`) would be a parallel path the data path has to consult, and the
pack must stay off by default exactly as ADR-0048 measured it. Second, the read side already has an
owner: ADR-0047 fixes the status object (`off`/`active`/`fallback`/`unknown`, per-tuple claims with an
evidence level, ADR-0024 metric fields) and is still Proposed.

## Decision

1. **One write route, four typed actions.** `POST /v1/policy/apply` on the local agent
   (bearer token, `--state-dir` and `--instance` required, as `POST /v1/tunnel/edit`):

   ```json
   {"confirm": true, "idempotency_key": "8-64 chars of A-Za-z0-9-_",
    "action": "enable" | "disable" | "select" | "reset", "profile": "gaming/1"}
   ```

   The request denies unknown fields and `action` is a closed set; an unknown action is 422.
   `profile` is required exactly for `select` and refused for the other three, so a typo cannot be
   mistaken for intent. `confirm` must be the literal `true` (ADR-0022 decision 2).

2. **Each action writes one of the existing inputs; no new one is added.**
   - `enable` writes the runtime switch and only that: the root helper creates
     `/etc/systemd/system/baft-tunnel@NAME.service.d/50-policy-pack.conf` containing exactly

     ```
     [Service]
     Environment=BAFT_TUNNEL_POLICY_PACK=sched
     ```

     `sched` and not `window-cap`: a profile's numbers are the scheduler pair, while `window-cap` is
     a measurement mode (ADR-0048 decisions 4-5). No profile is written, so enabling a tunnel with
     no configured profile runs the Balanced numbers, which are `sched`'s own defaults — TP-3.1
     recorded `balanced/1` as that identity.
   - `disable` removes that file, and only when its bytes are exactly the managed content. Any other
     content at that path, or the switch set in another drop-in or in the unit itself, is left
     untouched and the request is refused (`result: "refused"`, `cause:
     "switch_managed_elsewhere"`). The surface never edits operator-owned unit configuration.
   - `select` sets `policy.profile` in `/etc/baft-tunnel/NAME.json` to the given id. No other key
     changes; `policy.adaptive` is preserved.
   - `reset` removes `policy.profile` and `policy.adaptive` from that file, returning both to the
     schema defaults (absent profile = `balanced/1` identity, adaptive false), and leaves the switch
     exactly as it was. `reset` is not `disable`: `disable` is the gate, `reset` is the row.

   Validation is the runtime's own: the id must be one of the five accepted versioned ids
   (`balanced/1`, `gaming/1`, `trading/1`, `streaming/1`, `bulk/1`) and anything else is refused with
   that list; `select` on a `baft` tunnel is refused because `validate_policy` refuses the field for
   that engine (ADR-0031 decision 17). A refused request writes nothing.

3. **Capability gate.** `enable` and `select` require an installed binary built with `policy-pack`;
   otherwise 409 `feature_not_compiled` and nothing is written. This is an apply precondition read
   from the agent's own compile-time constant, not a status surface: without the feature the pack
   cannot run, so accepting the write would be a false record.

4. **Privileged boundary: the existing root manage helper (ADR-0055).** The agent is unprivileged
   (SA-09) and never writes `/etc/baft-tunnel` or `/etc/systemd`. It sends one typed request over the
   per-instance manage socket — `{op: "policy", key, action, profile?}`; no paths, no file contents,
   unknown fields refused — and the helper keeps its `SO_PEERCRED` plus exact-agent-cgroup
   authorization, so a same-UID agent of another instance is refused before any file operation. Then,
   in order:

   a. read the current `NAME.json` and (for the switch) the managed drop-in;
   b. build the result and validate it by deserializing into `RuntimeConfig` and running
      `validate_public`, which already applies `validate_policy` (accepted ids, engine scope);
   c. write the config atomically with its existing owner and mode, and the drop-in as root-owned
      0644, keeping the previous bytes in the per-key backup directory with a manifest (the
      `Op::Edit` mechanics of ADR-0055);
   d. `systemctl daemon-reload`;
   e. if the unit is running, restart it and wait for `active`;
   f. verify: the file parses and carries the requested `policy` object; with `enable`,
      `/proc/<MainPID>/environ` contains `BAFT_TUNNEL_POLICY_PACK=sched`; with `disable` it does not.

   Any failure after (c) restores both files to the bytes they had before the request — the config
   from its backup, and the drop-in to its previous content, recreated if it existed and removed only
   if it did not — reloads and restarts on those files. The receipt says `rolled_back` only when that
   restore, the reload and the restart all succeed, and lists the restored names. A stopped unit is
   never started (ADR-0055 decision 3): the receipt says `restart_required: true` and its
   verification is file-level only, labelled as such.

   If the restore, the reload or the restart on the previous files fails, the receipt is
   `recovery_failed`, not `rolled_back`: it names which step failed, the resulting file state it could
   and could not confirm, and the operator action the helper could not take (inspect the two named
   files, then `systemctl daemon-reload` and restart the unit). Nothing in this ADR may report a
   recovered state it did not verify.

5. **Idempotency is convergence, not repetition.** The same key with the same normalized request
   returns the stored receipt with `replayed: true`; the same key with a different request is 409
   (ADR-0022 decision 6). When the requested state already holds, the receipt is
   `result: "unchanged"` with no write and no restart, so running the command twice converges instead
   of failing or rewriting a file.

6. **Receipt and audit.** One receipt per idempotency key in `<state-dir>/receipts/<key>.json`
   (directory 0700, file 0640) and one audit line: `action: policy_apply`, `resource:
   policy/{enable|disable|select|reset}`, actor, target, request SHA-256, result and receipt id, in
   the ADR-0024 shape. The receipt carries `previous` and `requested` values, the names it touched
   (`NAME.json`, `50-policy-pack.conf`), `restart_required`, `verification` (unit-level or
   file-level with its reason) and `next_commands`. `result` is one of `configured` (written and
   verified), `unchanged` (the requested state already held), `refused` (nothing was written),
   `rolled_back` (the previous bytes, reload and restart were restored and verified) or
   `recovery_failed` (the restore did not verify; the receipt names the failed step and the operator
   action). It carries no file contents, no key material and no token.

7. **The CLI is a thin client of the same route.** `baft-tunnel policy enable|disable|select <ID>|
   reset --instance NAME [--agent URL] [--token-file PATH] [--json]`, defaulting to
   `http://127.0.0.1:18080` and the installed agent token path. It refuses a non-loopback URL before
   sending anything, sends the same fields, prints the receipt (text or `--json`) and shows the
   agent's cause verbatim. It performs no validation of its own, so there is exactly one
   interpretation of every request.

8. **The read side is deferred to ADR-0047, explicitly and with its reason recorded.** This ADR
   defines no `policy` object in `GET /v1/status`, no `GET /v1/policy`, no metrics fields, no
   `Unknown`/`Off` marking and no `policy status` CLI verb. Reasons:

   - ADR-0047 already owns that object and its rules (state set, per-tuple claim with an evidence
     level, ADR-0024 fields with provenance and freshness, CLI text form) and is Proposed; a shape
     defined here would either duplicate it or preempt its acceptance;
   - every profile claim in the pack is `simulated` only, and a read that cannot say `unknown` per
     tuple would misrepresent ADR-0031 decisions 10 and 14 and the project's no-fake-data rule;
   - #239's `Unknown`/`Off` marking was deferred to ADR-0047 for the same reason, and the pack's read
     should have one owner rather than three surfaces.

   Consequently the receipt never says "active". It records what was written (`configured`) and
   whether a restart was needed. "Is it running, and what does the running session use" is exactly
   ADR-0047's question.

9. **What TP-5.1 still owes (this ADR is its first step, not the item).** An accepted design ships
   no surface, so TP-5.1 stays open on two things, in this order:

   1. the implementation of the surface above — the `POST /v1/policy/apply` route, the helper's typed
      write with restart-verify and rollback, the thin `baft-tunnel policy …` CLI, and the tests named
      under "Evidence required" below;
   2. the read, after ADR-0047 is accepted: the status object and the `policy status` verb against
      it, which TP-5.2's panel card then consumes.

   Neither is blocked on LAB, and neither acquires a claim.

## Consequences

- The operator changes the pack through the same three gates the data path reads, with one receipt
  and one audit line, and the switch stops being an unmanaged unit edit.
- The manage helper gains a privileged write of one unit drop-in and a restart-verify step; its
  authorization is the ADR-0055 boundary and its rollback is the existing backup/revert mechanism.
- Nothing about the default path changes: with the feature off, or the switch unset, empty or `off`,
  the data path is today's, byte for byte. A profile alone never enables the pack; with the switch on
  and no profile the pack runs the Balanced numbers, as TP-3.1 recorded. ADR-0048's default-off
  decision and every `simulated` label stay exactly as they are.

## Rejected

- **A new `policy.enabled` config field.** It would be a fourth switch beside the feature and
  `BAFT_TUNNEL_POLICY_PACK`, and a parallel path the data path would have to consult.
- **A `window-cap` mode in the surface.** That is the ADR-0048 measurement knob, not an operator
  intent, and exposing it invites tuning without evidence.
- **Letting the agent write the unit file or `/etc/systemd` itself.** It is unprivileged by design
  (SA-09); ADR-0055's helper boundary exists for exactly these writes.
- **Shipping the status read here.** It would preempt ADR-0047 and, with `simulated`-only evidence,
  invite a "working profile" claim the project forbids.

## Not covered (stated, not faked)

- The status and metrics read, and every `Unknown`/`Off` marking (ADR-0047); the panel card (TP-5.2);
  the pack's documentation page (TP-5.3); the UDP carrier (#244).
- Turning the adaptive controller on. `select` preserves `policy.adaptive` and `reset` clears it, but
  no action sets it, because ADR-0056's opt-in has its own LAB criterion and is not an operator
  toggle before that is answered.
- A receipt for a unit the agent does not manage: without `--instance` the route is 409, as
  `tunnel/edit` already is.

## Evidence required to call the implementation done

Unit tests on the agent route (typing, unknown action and unknown field, `profile` required only for
`select`, accepted-id list, engine-scope refusal, feature gate, `unchanged` no-op, replay and key
conflict, missing `--instance`/state dir) and on the helper (managed drop-in written and removed,
refusal when the path or the switch belongs to someone else, atomic replace with backup and revert,
file-level verification for a stopped unit, unit-level verification with the restart). The
default-build test that the data path is unchanged with no switch and no profile already exists and is
not replaced by this work. No measurement is added by an operator surface, so no evidence label
changes.
