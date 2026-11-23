# ADR-0055: remove and edit a created tunnel from the Agent API

Status: accepted feature design; security amendment proposed 2026-11-22 in PR #416. Builds on ADR-0023 (control helper),
ADR-0026 (root setup helper), ADR-0036 (one agent per instance). The amended privileged boundary is not merge-approved
until independent review and real-systemd verification are complete.

## Context

The Agent API could create a tunnel (setup apply) and start, stop or restart it, but not remove it or change it. The
panel showed an honest notice instead (#396). Removal and edit change root-owned files in `/etc/baft-tunnel`; the agent
runs unprivileged (SA-09) and must stay unable to write configuration or read keys.

## Decision

1. Original implementation (superseded by the 2026-11-22 security amendment below): the agent wrote
   `/run/baft-tunnel/control/NAME.manage`; `baft-tunnel-control-manage@NAME.path` ran exactly
   `baft-tunnel setup manage-helper --instance NAME` (oneshot,
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
   only; key material, file contents and the token are never written. The newest 3 edit backups per instance are
   kept; remove backups (the only copy of a removed key) are never deleted automatically.

## Pre-merge security findings and remediation (2026-11-22)

A source-level review of the original file-based implementation found a **High cross-instance confused-deputy issue**:
all agents shared one Unix account and writable control directory, while the root helper trusted the instance encoded in
the systemd unit name. A compromised agent A could therefore create `B.manage` and ask the root helper for B to remove,
edit, or revert B's root-owned configuration and pairing/key files.

The proposed amendment above moves the authorization decision to the privileged boundary. It uses kernel-provided
`SO_PEERCRED` plus the exact systemd agent cgroup; a same-UID peer from another instance is refused before any file
operation, including `revert`. The adversarial unit test models A attempting `remove`, `edit`, and `revert` against B with the same UID and proves
B's config and backup state remain unchanged. Cgroup v1/v2 parsing, UID/GID mismatch and near-match cgroup paths are
also covered. The real-systemd lifecycle harness now includes a two-instance probe that runs inside A's exact agent
service cgroup, connects to B's root socket, and verifies refusal plus unchanged B config/backup state. `sub_key()` now derives a full 256-bit SHA-256 digest from the complete caller key and lifecycle step,
removing the previous 54-character truncation collision.

**Verification boundary:** the real-systemd two-instance probe is added but has not been executed in this session. Its
Linux CI result on Ubuntu 22.04 / systemd 249 is still required before merge; source/unit tests alone do not prove the
privileged boundary. No LAB, production host, install, service, release, or deploy operation was performed in this
session. No independent reviewer has approved this remediation yet.

The inherited shared-account assumptions for the other file-based helpers are explicitly scoped in ADR-0023 and
ADR-0026; this amendment does not claim to authenticate those older helper verbs.
## Not claimed

Evidence currently available: helper unit tests on temp directories and agent route tests with fake units and an
in-process helper (remove, refusal with restart, edit, failed-restart rollback, gates, replay). The real-systemd
two-instance probe is scripted but not yet run; no LAB run is claimed. The peer node is never changed by either route. Panel UI follows in the React panel thread after this merges.

## 2026-11-23 amendment — per-instance manage-socket filesystem boundary

The shared baft-tunnel-agent UID is not sufficient as the filesystem authorization boundary for per-instance manage sockets. Each instance now receives a dedicated system group named btm- plus the first 16 hexadecimal characters of SHA-256(instance-name). This keeps the Linux group name short even when the valid instance name is 63 characters.

The root-owned socket unit template is fail-closed (SocketGroup=root, SocketMode=0600). Registration and the upgrade-boundary migration create root-managed per-instance drop-ins that set the socket group to that instance's dedicated group and mode 0660, and add that same group only to the matching baft-tunnel-agent@INSTANCE.service through SupplementaryGroups. Other instance agents retain their own disjoint groups. The shared agent UID therefore cannot connect to another instance's socket merely because it owns the same UID.

SO_PEERCRED and exact systemd cgroup verification remain mandatory defense-in-depth checks inside the root helper. The installer verifies that a derived group has no explicit members and is not a primary group for any account before using or deleting it. When an upgrade creates or changes an instance's access drop-in, only that instance's active agent service is restarted so its supplementary group set takes effect; tunnel services are not restarted. Instance removal and uninstall remove managed drop-ins and delete only empty derived groups.

The real-systemd lifecycle test now attempts 80 cross-instance socket connections from one agent's exact service cgroup and requires all to fail at the kernel socket-permission boundary without changing the other instance's helper invocation ID or configuration. This is test coverage, not yet Linux CI or LAB evidence: it must pass on the project's supported systemd runner before this amendment is accepted. No CAP_NET_ADMIN is required or granted.
