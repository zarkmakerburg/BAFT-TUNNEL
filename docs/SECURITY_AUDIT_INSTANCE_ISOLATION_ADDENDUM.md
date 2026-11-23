# S5 security audit addendum — instance isolation (ADR-0055) and uninstall fail-closed

- Date: 2026-11-23.
- Baseline audit: [SECURITY_AUDIT_PRE_STABLE.md](SECURITY_AUDIT_PRE_STABLE.md) (passes 1–5, last audited main `05aa2dc`), plus [SECURITY_ASSESSMENT.md](SECURITY_ASSESSMENT.md) (SA-01..SA-11).
- Reviewed delta: the security-relevant merged changes after that baseline, specifically:
  - PR #416 (squash `8a5dd13`) — per-instance manage socket, root tunnel helper, and `agent.rs` manage routes (ADR-0055).
  - PR #426 — uninstall fail-closed systemd inventory. **Correction:** the #426 squash (`7fa2d9a`) is byte-identical to its parent `8a5dd13`; the uninstall work did not land and is re-landed separately (see below).
- Method: manual source review of the changed files, targeted by component. No host or LAB run was performed for this addendum. CI evidence is hosted-runner only.

## Independence and limits

This addendum was produced by an agent that contributed to PR #416 (a compile fix in `tunnel_helper.rs`, a test-permission fix, and the ADR-0055 documentation). It is therefore **not an independent human review** and must not be presented as one. It is a technical source review and does not by itself close S5. Items marked "not re-read" below are not counted as clean.

## Components reviewed

| Component | What was read | Depth | Dynamic |
|-----------|---------------|-------|---------|
| `src/tunnel_helper.rs` | `authorize_peer`, `expected_agent_ids`, `peer_cgroup`, `peer_credentials`, `systemd_listener`, `read_stream_bounded`, `process_request`, `handle_request`, `serve`, `op_remove`/`op_edit`/`op_revert` windows, `submit_in`, `SystemManageHelper` | Full for the listed functions | No (unit tests/CI only) |
| `systemd/control/baft-tunnel-control-manage@.service` | Full unit: capability set, `Protect*`, `RestrictAddressFamilies=AF_UNIX`, `NoNewPrivileges` | Full | No |
| `systemd/control/baft-tunnel-control-manage@.socket` | `SocketUser/Group/Mode=0600`, `Accept=no`, fixed per-instance path | Full | No |
| `scripts/install-agent-control.sh` | `manage_group_for_instance`, `ensure_manage_access`, `remove_manage_access`, legacy path retirement, socket enable/restart | Targeted (manage portions) | No |
| `src/agent.rs` | `tunnel_manage` (instance from `ctx.instance`), `manage_steps`, replay/confirm gates | Targeted | No |

## Result

Within the reviewed delta: **no Critical, High or Medium finding.** Two Info/accepted items.

| ID | Severity | Status | Title |
|----|----------|--------|-------|
| S5-D1 | Info | Accepted (by design; same root cause as SA-11) | Instance isolation is enforced by per-instance supplementary group (DAC) plus exact systemd cgroup; all agents share one account |
| S5-D2 | Info | Accepted | `peer_cgroup` reads `/proc/<pid>/cgroup` by PID after `SO_PEERCRED`; PID reuse is mitigated by re-reading and comparing before/after the request |

### S5-D1 — shared account, group + cgroup boundary

All agents run as the single `baft-tunnel-agent` account, so the UID/GID check alone cannot distinguish instances. The actual boundary is:

1. Each instance gets a dedicated group; the agent unit drop-in adds it as `SupplementaryGroups` and the socket drop-in sets `SocketGroup=<group>` / `SocketMode=0660`, so cross-instance connects fail DAC.
2. Even if DAC were bypassed, the helper requires the exact cgroup path `/system.slice/baft-tunnel-agent@{name}.service` (`authorize_peer`), read from `SO_PEERCRED`'s PID.

`install-agent-control.sh` refuses a per-instance group that is non-empty or a primary group, and refuses to remove one that is still in use. Residual: this is a point-in-time check, not continuous monitoring of `/etc/group` or unit drop-ins; a privileged host administrator who later changes group membership or the drop-in is outside the threat model, identical to SA-11. No code change recommended.

### S5-D2 — PID-based cgroup read

`SO_PEERCRED` yields the peer PID; `/proc/<pid>/cgroup` is then read. If that PID exits and is recycled, the helper could read a different process's cgroup. `serve()` re-reads the cgroup after the request is fully read and requires it to equal the pre-read value, failing closed on a change. Combined with the socket being drained only after the peer is authorized, the practical window is negligible and a mismatch is refused. Accepted.

## Verified without findings (reviewed delta)

- **Instance is not attacker-chosen.** `tunnel_manage` takes `name` from `ctx.instance` (`--instance`), never from the request body; `handle_request`/`serve` also reject an invalid instance name and `authorize_peer` repeats the check.
- **Peer authentication before reading.** `serve()` obtains `SO_PEERCRED`, resolves expected agent uid/gid, reads the peer cgroup, and calls `authorize_peer` **before** reading any client bytes, so an agent from another instance cannot hold the root helper open.
- **Socket activation is bound to this unit.** `LISTEN_PID` must equal the helper PID, `LISTEN_FDS` must be exactly 1, `LISTEN_FDNAMES` must be `manage`, the passed socket's pathname must equal the expected instance path, and `SO_ACCEPTCONN` must be 1.
- **Bounded, strict request.** At most 64 KiB read with timeouts; `serde` `deny_unknown_fields`; `valid_key` and `valid_instance` enforced; peer addresses accepted only for `edit`.
- **No agent-supplied paths or contents.** The helper derives every path from the instance name; the agent sends no file paths or contents. Receipts list file names only, never key material or contents (asserted by tests).
- **File handling.** `O_NOFOLLOW` + regular-file checks for reads; `remove` moves only the instance's own config and pairing files (guarded by `pairing-state.json`) into a root-only `0700` backup with key `0600`; operator PKI files (`node.key`, `node.crt`, `ca.crt`, `ca.crl`) are never touched; `revert` is compare-and-swap on the edit's own written hashes and refuses over a later change; a failed remove keeps its backup.
- **Unit hardening.** `baf-tunnel-control-manage@.service` has `NoNewPrivileges`, `PrivateTmp/Devices/Network`, `ProtectSystem=strict` with narrow `ReadWritePaths`, `RestrictAddressFamilies=AF_UNIX`, and `CapabilityBoundingSet=CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE` only.
- **Privilege escalation surface.** The socket is `root:root` by default (`0600`); the per-instance `0660` is applied by a managed drop-in. Only the matching agent's supplementary group can connect; the helper never executes a shell and takes no path from the request.
- **A→B adversarial probe (CI).** On the hosted Ubuntu 22.04 / systemd 249 runner, 80 cross-instance connect attempts were denied before helper activation, with no change to the target helper invocation ID, target config digest, or stray backup (phase1-rust run 45249141966).

## Not re-audited here (not counted as clean)

The whole tree since the last audited main `05aa2dc` was not re-read in this addendum. Components not named above (transport, `release.yml`, other panel/agent routes changed after `05aa2dc`, LAB scripts) are **not** covered by this addendum and must not be inferred clean from it. A whole-system re-audit against the current `main` remains part of S5.

## Evidence

- PR #416 squash: `8a5dd13d7387ac5ec13ac7e502ccf33b8a02aca1`; managed A→B probe in phase1-rust run 45249141966 (all jobs SUCCESS).
- `scripts/pki/test-agent-control-lifecycle.sh` exercises the cross-instance denial on real systemd (hosted runner), not LAB.
- Evidence level: source review + hosted CI. No two-host LAB and no Production acceptance.
