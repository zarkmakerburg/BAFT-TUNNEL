# ADR-0023: Agent control helper (service start through a systemd path unit)

Status: accepted (owner chose "path-unit helper" on 2026-11-08 after polkit proved unusable).
Builds on ADR-0022 (Apply, Verify, Receipt) and ADR-0016 (Ubuntu 22.04 / systemd 249).

## Context

The agent and the tunnel unit run as the unprivileged `baft-tunnel` user with
`NoNewPrivileges`. Starting a unit needs a privilege grant. Polkit 0.105 (Ubuntu 22.04) only
supports `.pkla` files, which cannot limit a grant to `baft-tunnel@*` units, so a polkit rule would
let the agent start any unit. sudo is blocked by `NoNewPrivileges`.

## Decision

1. A root-owned helper made of systemd units, installed by `scripts/install-agent-control.sh
   --instance NAME` (explicit, per instance, never by default):
   `baft-tunnel-control-{start,stop}@NAME.path` watches `/run/baft-tunnel/control/NAME.{start,stop}`
   and triggers `baft-tunnel-control-{start,stop}@NAME.service`, a oneshot whose only command is
   `systemctl --no-block {start,stop} baft-tunnel@NAME.service`, then removes the request file.
2. `/run/baft-tunnel/control` (tmpfiles.d) is `root:baft-tunnel 0770`: the only place the agent can
   write. The agent can only cause two fixed commands on units named `baft-tunnel@NAME`, and
   `NAME` must have a path unit enabled for it and `/etc/baft-tunnel/NAME.json` present.
3. Apply gets a `start` stage: gates on a valid configuration, direction support and the unit not
   already running; writes the request; waits up to 20 s; verifies `unit_active`, `stable_3s`
   (no restart, no failure), `listener_accepting` and, for Direct forward nodes, `path_probe` (a
   connection to the ingress stays open 2 s). If verification fails it requests `stop` and records
   `rolled_back`. Reading unit state needs no privilege (`systemctl show`).
4. Instance names for this path are limited to `A-Za-z0-9_-`.

## Not done (stated in every receipt)

- Not enabled at boot, no firewall change or inspection, peer not verified from here, data
  integrity is not tested by Apply (the LAB tests cover it). `path_probe` shows a stream was
  accepted, not that bytes arrive intact.

## Evidence

Unit tests with a fake unit controller (success, failed start with stop, refusals, request
files), staging run of the install script. **Not yet run on a real systemd host**: path-unit
triggering, tmpfiles permissions and the 20 s start window need the LAB.
