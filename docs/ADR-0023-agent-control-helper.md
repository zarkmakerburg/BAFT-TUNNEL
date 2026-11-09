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
   (no restart, no failure) and `listener_accepting`. If verification fails it requests `stop` and
   records `rolled_back`. A connection-held probe was tried and removed: the first real-systemd CI
   run showed a forward node holds a local connection open even with no peer, so it proved nothing. Reading unit state needs no privilege (`systemctl show`).
4. Instance names for this path are limited to `A-Za-z0-9_-`.

## Amendment 1 (2026-11-09, U3): stop, restart and boot persistence

Two more fixed commands, same mechanism (request file -> root path unit -> one `systemctl` command
for exactly `baft-tunnel@NAME`): `baft-tunnel-control-enable@NAME` runs `systemctl --no-block enable --now`
and `baft-tunnel-control-disable@NAME` runs `systemctl --no-block disable --now`. Apply stage `start`
now requests `enable` (started now and after every reboot) and verifies `UnitFileState=enabled`; a
start that fails verification is rolled back with `disable` (stopped and not started at boot). New
stages `stop` (requests `disable`, verifies inactive and not enabled) and `restart` (requests `stop`,
waits for inactive, then `enable`; verifies a new main PID). The same three are offered without a
setup plan for the instance the agent is bound to: `POST /v1/tunnel/lifecycle`
(`{"action":"start|stop|restart","confirm":true,"idempotency_key":"..."}`), used by the panel's
Tunnels page. Receipts state `boot.enabled_at_boot`; a reboot itself is not performed or claimed.
The agent runs as its own user `baft-tunnel-agent` (SA-09) from the shipped unit
`baft-tunnel-agent@NAME.service`, started by `baft-tunnel-agent@NAME.path` once `NAME.json` exists
and enabled at boot; `install.sh --register NAME` (or `install-agent-control.sh --instance NAME`)
installs all of it in one step.

## Not done (stated in every receipt)

- (Before Amendment 1: not enabled at boot.) No firewall change or inspection. The running unit writes a session
  indicator (`$RUNTIME_DIRECTORY/session.json`, no secrets, bound to its main PID) while an
  authenticated and authorized session exists; Apply `start` records it as `observed`, otherwise
  `unknown`. `data_path_integrity` and `ready` stay `unknown` in every Apply receipt: only a test
  that moves bytes through the tunnel (P0-5) can observe them.

## Evidence

Unit tests with a fake unit controller (success, failed start with stop, refusals, request
files), staging run of the install script. Real systemd 249 (hosted Ubuntu 22.04) CI run: `scripts/pki/test-agent-control-lifecycle.sh`.

### Real-systemd evidence (hosted ubuntu-22.04, systemd 249, `ubuntu-2204-baseline`)

- P0-1 gap tests: malformed/unregistered names change no unit, malformed names refused by the API,
  duplicate start does not restart, symlinked request removed without touching its target,
  consumed stop not replayed, firewall rules/listeners/unrelated services unchanged.
  Run 42523536944, code SHA 22e94f1, merged as 14fd71f (#90).
- P0-2: two nodes with their own certificates on one host: serve alone stays `unknown`; with the
  forward node the session is `observed`; 64 KiB and 2 MiB round-trip unchanged; the indicator
  clears when the peer stops. Run 42524638625, code SHA 231ba9d, merged as e031475 (#93).
- P0-2 two-host: P0-5 LAB run 42538458094 (main 92c4e70, signed rc.4): Apply start on setup-written configs on both hosts, `receipt_session=observed`, `receipt_ready=unknown`, 64 KiB + 2 MiB integrity measured by the stage, recovery, occupied-port rollback, cleanup.
- Limit: one host, loopback, hosted runner. The two-host LAB proof is part of P0-5.
