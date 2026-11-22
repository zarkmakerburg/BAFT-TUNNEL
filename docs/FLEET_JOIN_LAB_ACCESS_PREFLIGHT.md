# Fleet Join LAB — read-only access preflight

This workflow is dedicated to the two existing, owner-registered Fleet Join LAB VMs (Hub in Iran and Member abroad). Their observed mixed Ubuntu versions are documented in [the mixed Ubuntu compatibility plan](FLEET_JOIN_MIXED_UBUNTU_COMPATIBILITY.md). Do not provision replacement VMs or reimage these hosts under that plan. It uses the separate GitHub Actions Environment `baft-tunnel-fleet-lab-20261121`; it must not reuse or modify the existing `baft-tunnel-lab` environment.

## Environment contract

Secrets:
- `LAB_IRAN_HOST`
- `LAB_ABROAD_HOST`
- `LAB_SSH_PRIVATE_KEY`
- `LAB_SSH_KNOWN_HOSTS` — pinned ED25519 host keys for both hosts. Strict `known_hosts` matching confirms that SSH presented a configured key; it does **not** independently establish the host's identity or prove that the key matches the provider-console fingerprint. Complete that out-of-band cross-check separately before treating host identity as verified.

Variables:
- `LAB_IRAN_SSH_USER`
- `LAB_ABROAD_SSH_USER`
- `LAB_IRAN_SSH_PORT`
- `LAB_ABROAD_SSH_PORT`
- `LAB_TEST_PORTS`

Keep host addresses, private keys, and known-hosts contents inside this GitHub Environment. Never copy them into repository files, issues, artifacts, or logs. Do not retrieve or print secret values.

## Run conditions

Workflow: `.github/workflows/fleet-join-lab-preflight.yml` (`Fleet Join LAB access preflight (manual, read-only)`).

- Trigger: `workflow_dispatch` only.
- Ref: `main` only.
- Confirmation input: exactly `FLEET_JOIN_READ_ONLY_PREFLIGHT`.
- Environment: `baft-tunnel-fleet-lab-20261121`.
- Concurrency: shares the Phase 0 two-host concurrency group so this workflow cannot overlap the existing two-host SSH workflow.

## Scope and evidence limits

The workflow reuses `lab/real-hosts/preflight-two-hosts.sh`. It verifies SSH access using strict pinned host-key checking, reads OS/kernel/architecture/init and required-tool availability on both hosts, then probes configured TCP ports from Iran to abroad. It creates SSH key and known-hosts files only in a unique runner-temporary directory and removes that directory with an EXIT trap.

This is an access/reachability preflight only. It does **not** inspect all BAFT-TUNNEL paths, systemd versions/units, users/groups, or Fleet CA state; it does not establish that the hosts are disposable or independently verify their SSH host keys against the provider console. The observed Ubuntu 24.04.4 / Ubuntu 22.04.4 pair is an owner-approved mixed-version test target, not a change to the product's Ubuntu 22.04 / systemd 249 baseline. See [the compatibility plan](FLEET_JOIN_MIXED_UBUNTU_COMPATIBILITY.md) for staged gates and evidence limits. It does not verify a candidate binary SHA, install software, initialize a CA, run Fleet Join, start services/tunnels, change firewall/routing/sysctl, or reboot either host. A successful run is not Fleet Join acceptance evidence.

Before any real-host run, re-read the latest Issue #2 comments, ensure no other LAB operation is active, and announce the exact workflow/ref and read-only scope on Issue #2. This PR does not dispatch the workflow or authorize a real-host connection.
