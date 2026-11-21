# ADR-0055: remove and edit a created tunnel from the Agent API

Status: accepted (2026-11-21, owner's delegated decision authority; owner asked why created tunnels cannot be edited or
deleted). Builds on ADR-0023 (control helper), ADR-0026 (root setup helper), ADR-0036 (one agent per instance).

## Context

The Agent API could create a tunnel (setup apply) and start, stop or restart it, but not remove it or change it. The
panel showed an honest notice instead (#396). Removal and edit change root-owned files in `/etc/baft-tunnel`; the agent
runs unprivileged (SA-09) and must stay unable to write configuration or read keys.

## Decision

1. A new root helper, same mechanism as ADR-0026: the agent writes `/run/baft-tunnel/control/NAME.manage`;
   `baft-tunnel-control-manage@NAME.path` runs exactly `baft-tunnel setup manage-helper --instance NAME` (oneshot,
   no network, `ProtectSystem=strict`, capabilities limited to `CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE`; no
   `CAP_NET_ADMIN`). The request carries only `op`, an idempotency key and, for edit, IP addresses. The agent never
   sends file contents or paths; unknown fields are refused. Installed by `install-agent-control.sh` (verb `manage`).
2. `POST /v1/tunnel/remove` `{"confirm":true,"confirm_name":"NAME","idempotency_key":"..."}`: the name must be typed
   again. The agent stops and disables the unit (ADR-0023 stop stage, verified), then the helper, which refuses while
   `/run/baft-tunnel/NAME` exists, moves `NAME.json` and, only when `pairing-state.json` names `NAME.json`, the pairing
   state, bundles and generated Noise key into a root-only backup `/var/lib/baft-tunnel-control/tunnel-backups/NAME/KEY`
   (0700, files keep 0600/0640). Operator PKI files (`node.key`, `node.crt`, `ca.crt`, `ca.crl`) are never touched. A
   failed move is undone file by file; if the helper does not remove, the agent starts the unit again when it was
   enabled before. On success the agent notices `NAME.json` is gone and restarts into setup mode, so the panel can
   create a new tunnel. The agent and helper units stay installed.
3. `POST /v1/tunnel/edit` `{"confirm":true,"idempotency_key":"...","peer_host":"IP","backup_peer_hosts":["IP"]}`: only
   the dialing (forward) side's `remote` and `backup_remotes` change; the port, identities, keys, engine, direction,
   listeners and ports stay. Addresses must be unicast, not loopback or unspecified, distinct, at most 3 backups. The
   helper validates the result against the runtime schema (`validate_public`), keeps a backup, and replaces the files
   atomically with the same owner and mode (pairing state is updated too when it belongs to the instance). A running
   unit is restarted and verified; if that fails the helper restores the backup (`revert`) and the unit is brought up
   on the old configuration (`rolled_back`). A stopped unit is not started. The agent reloads when `NAME.json` changes.
4. Both routes: bearer token, state directory and `--instance` required, idempotency with replay and conflict,
   receipt in `receipts/`, one audit line (`tunnel_manage`, `tunnel/remove|edit`). Receipts and audit hold file names
   only; key material, file contents and the token are never written. The newest 3 backups per instance are kept.

Rejected: letting the agent write configuration (breaks SA-09); deleting without a backup (no rollback); editing ports
or identities (needs the peer to change in step; a later pairing-level change, not a local edit).

## Not claimed

CI only: helper unit tests on temp directories and agent route tests with fake units and an in-process helper
(remove, refusal with restart, edit, failed-restart rollback, gates, replay). No real-systemd or LAB run yet; the peer
node is never changed by either route. Panel UI follows in the React panel thread after this merges.
