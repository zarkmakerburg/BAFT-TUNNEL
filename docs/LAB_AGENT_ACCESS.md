# LAB Agent Access — start here, do not scan the repository

This is the single entry point for an incoming agent who needs to inspect or connect to the BAFT-TUNNEL real-server LAB.

## 1. Read only these sources first

1. [HQ Control Board, Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) — read the issue and the **latest comments** for current host status, active operations, exact branch/SHA, and safety constraints.
2. This file — connection contract and the read-only entry point.
3. [Two-host LAB inventory and environment contract](PHASE0_TWO_HOST_LAB.md) — authoritative variable/secret names.
4. [Read-only preflight workflow](../.github/workflows/phase0-two-host-lab.yml) — exact connection behavior.

Do **not** recursively browse the repository, search all workflows, or infer addresses from old logs. The active LAB is exactly two hosts: **Iran** and **abroad**. The proposed six-host matrix is not active.

## 2. Where the connection data lives

GitHub repository: `zarkmakerburg/BAFT-TUNNEL` (private)

GitHub Actions Environment: `baft-tunnel-lab`

Environment variables (hostnames/IPs and SSH connection metadata):
- `LAB_IRAN_SSH_USER`
- `LAB_IRAN_SSH_PORT`
- `LAB_ABROAD_SSH_USER`
- `LAB_ABROAD_SSH_PORT`
- `LAB_TEST_PORTS`

Environment secrets (the repository is public, so host addresses must be secrets: variables are printed in public workflow logs):
- `LAB_IRAN_HOST`
- `LAB_ABROAD_HOST`
- `LAB_SSH_PRIVATE_KEY` — dedicated LAB SSH private key.
- `LAB_SSH_KNOWN_HOSTS` — independently verified pinned host keys for both hosts.

The actual values intentionally remain in the private GitHub Environment. **Do not ask the owner to paste them into chat; do not copy them into repo files, issues, artifacts, shell output, or logs.** GitHub Actions secrets are write-only to normal repository/API readers; an agent should not attempt to retrieve or print their values.

## 3. The supported connection path (read-only)

Use the existing manual workflow as the SSH access broker. It creates short-lived runner files from the Environment secrets, enforces strict SSH host-key checking, connects to both hosts, performs read-only inspection and TCP reachability probes, then deletes the temporary key material even on failure.

GitHub UI steps:
1. Open [Actions → Phase 0 two-host LAB (manual, read-only preflight)](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/workflows/phase0-two-host-lab.yml).
2. Select **Run workflow**.
3. Select branch `main`.
4. Enter exactly `READ_ONLY_PREFLIGHT` in the `confirm` input.
5. Start the workflow and inspect the run summary/logs for both host reports.

Equivalent GitHub CLI command (if authenticated with access to this private repository):

```bash
gh workflow run phase0-two-host-lab.yml \
  --repo zarkmakerburg/BAFT-TUNNEL \
  --ref main \
  -f confirm=READ_ONLY_PREFLIGHT

gh run list --repo zarkmakerburg/BAFT-TUNNEL \
  --workflow phase0-two-host-lab.yml --limit 3
```

Then inspect the run ID returned by `gh run list`:

```bash
gh run view RUN_ID --repo zarkmakerburg/BAFT-TUNNEL
gh run view RUN_ID --repo zarkmakerburg/BAFT-TUNNEL --log
```

Do not paste raw logs into public locations. Check that the run completed successfully and that both Iran and abroad reports are present. Treat output as evidence from that run only; do not infer tunnel readiness from an SSH preflight.

## 4. What this workflow does — and does not do

It reads OS/kernel/architecture/tool availability and probes configured TCP ports from Iran to abroad. It does **not** install packages, write persistent remote files, change firewall/routing/sysctl, restart services, start/stop tunnels, or reboot hosts.

This is the correct first connection for an incoming agent who only needs to identify the LAB hosts and verify SSH access.

## 5. If the task requires commands beyond read-only inspection

The read-only workflow is not a general-purpose remote shell. Do not repurpose it or bypass its confirmation gate.

Before any state-changing LAB operation:
- Read the latest HQ handoff and verify the exact task, target host(s), and current state.
- Use a dedicated, narrowly scoped, manually triggered workflow with explicit confirmation, strict pinned host keys, masked host values, bounded timeout, and unconditional ephemeral-key cleanup.
- Make the operation reversible, capture before/after evidence, and define cleanup/abort criteria.
- Never reboot, upgrade the OS/systemd, alter production, or touch unrelated services as an implicit part of a test.
- For P1-000, use an isolated instance and dedicated temporary paths/ports; do not overwrite the existing BAFT-TUNNEL service or user data. The active LAB baseline is Ubuntu 22.04 / systemd 249.

If no appropriate scoped workflow exists, report the precise missing capability in HQ and create that workflow first; do not improvise direct access or expose credentials.

## 6. Required handoff after access

Append a concise comment to [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) recording:
- workflow run URL and exact commit/ref;
- which host checks completed (label hosts Iran/abroad; do not disclose addresses);
- read-only vs state-changing scope;
- actual observed results and any failures;
- confirmation that ephemeral credentials were cleaned;
- next executable action and any blocker.

Never claim an SSH connection, test, or cleanup succeeded unless the corresponding workflow step actually completed successfully.
