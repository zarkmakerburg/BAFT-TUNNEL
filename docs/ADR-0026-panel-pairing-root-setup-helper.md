# ADR-0026: panel pairing through a root setup helper (config write boundary)

Status: accepted (2026-11-10, under the owner's delegated decision authority).
Builds on ADR-0022 (Apply, Verify, Receipt), ADR-0023 and its amendment (agent control helper), SA-09 (agent user).
Plan: REMAINING_WORK_PLAN.md U4 and U5b; closes P1-8 "Add from Link" as the panel bundle import.

## Context

On an installed host the agent runs as `baft-tunnel-agent` and cannot write `/etc/baft-tunnel`, so the panel's Apply for
`init`/`join` was blocked (Q3) and the operator ran the printed root commands. The agent also needed `NAME.json` before it
started at all, so the panel could not help before the configuration existed.

## Decision

1. **Setup mode.** `baft-tunnel-agent@NAME` runs from registration on. When `/etc/baft-tunnel/NAME.json` does not exist it
   starts in setup mode: only status (reduced, `setup_pending: true`), engines, audit and the setup routes answer; other
   routes return 409. Once the file exists (panel or CLI) the agent exits and `Restart=always` brings it back with the
   configuration loaded.
2. **Root setup helper.** For the bound instance and directory only, Apply `init`/`join` writes one request file
   `/run/baft-tunnel/control/NAME.setup`. `baft-tunnel-control-setup@NAME.path` runs the fixed command
   `baft-tunnel setup helper --instance NAME` as root. It reads the request without following links (O_NOFOLLOW, regular
   file, at most 64 KiB), removes it, and refuses any stage other than `init`/`join`, another instance, another
   directory, unknown fields, or a join without bundle and checksum.
3. **No agent-written content is installed.** Instead of staging finished files for root to copy (the plan's first
   sketch), the request carries only the plan parameters, the public peer bundle and its checksum. The helper derives every
   file with the same code as the CLI (`apply::run` -> `setup::init`/`setup::join`), with the same verification, rollback of
   only what it created, refusal to overwrite, and the fixed service owners and modes (`setup::adopt_for_service`). This is a
   narrower boundary than validate-and-copy: the agent cannot choose file content, paths or modes.
4. **Result.** The receipt goes to `NAME.setup-result` (root:baft-tunnel 0640, created exclusively after removing any
   stale file or planted link). The agent accepts it only when it carries the request's idempotency key and digest, stores
   its own copy (same-key replay) and writes an audit line. The helper keeps its own receipts in
   `/var/lib/baft-tunnel-control/setup-receipts/NAME`.
5. **Panel.** The Wizard's last step pairs from the panel when the agent is in setup mode: prepare (init), export the own
   bundle (copy, download, or a `baft-tunnel://pair?v=1&b=<base64url>` link) with its checksum, import the peer bundle
   (paste JSON or link, or upload) with a preview from `POST /v1/setup/bundle/inspect` (checksum compare, field match against
   this node, changes nothing), join only when both match and the operator confirms, then Start. The link never carries the
   checksum, which must be compared over a separate channel.
6. Preflight replaces `directory_writable` with `setup_helper` for the bound instance. The printed root commands stay as
   an alternative.

## Consequences

- Panel-complete pairing for `noise-tcp`, and for `tcp-tls` when the PKI files are already in `/etc/baft-tunnel`
  (`baft-pki.sh`). CSR download and certificate upload in the panel (D2) are not part of this ADR.
- Group `baft-tunnel` can request setup for a registered instance, as it can already request start/stop (ADR-0023). The
  tunnel user is in that group; a compromised tunnel process could ask for an `init`/`join` that the helper validates
  exactly like the CLI and that never overwrites. Accepted with SA-09; revisit with the dedicated control group.
- Evidence: unit tests (`setup_helper`, agent pairing through the in-process helper), `scripts/pki/test-panel-pairing.sh`
  on real systemd in CI (tamper, symlink, oversize and scope refusals, planted result link), `scripts/panel-qa/panel-pairing.js`
  (fa/en). Two-host LAB run that sets up both nodes through the agent API only: run 42779915532 PASS (main `4887a72`, 2026-11-10).
