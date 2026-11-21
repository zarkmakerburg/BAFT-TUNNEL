# LAB Agent Access — start here, do not scan the repository

This is the entry point for an incoming agent who needs to inspect or connect to a BAFT-TUNNEL real-server LAB. Read the latest [HQ Control Board, Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) comments before any LAB operation.

## 1. Select exactly one LAB

BAFT-TUNNEL has two separate two-host LAB configurations. They have different host pairs, GitHub Actions Environments, and workflows. **Never substitute one for the other.** The old six-host matrix is not active.

| LAB | Intended use | GitHub Actions Environment | Entry-point documentation | Read-only workflow |
|---|---|---|---|---|
| Established Phase 0 LAB | Tasks explicitly scoped to the original two-host Phase 0 LAB | `baft-tunnel-lab` | [PHASE0_TWO_HOST_LAB.md](PHASE0_TWO_HOST_LAB.md) | [phase0-two-host-lab.yml](../.github/workflows/phase0-two-host-lab.yml) |
| Dedicated Fleet Join LAB | Fleet Join investigation and its separately gated acceptance plan | `baft-tunnel-fleet-lab-20261121` | [FLEET_JOIN_LAB_ACCESS_PREFLIGHT.md](FLEET_JOIN_LAB_ACCESS_PREFLIGHT.md) | [fleet-join-lab-preflight.yml](../.github/workflows/fleet-join-lab-preflight.yml) |

For Fleet Join, use only the dedicated Fleet Join documentation and workflow. Do not reuse or modify `baft-tunnel-lab`. The dedicated workflow shares a concurrency group with the Phase 0 workflow so the two real-host SSH workflows cannot overlap.

## 2. Repository and credential handling

GitHub repository: `zarkmakerburg/BAFT-TUNNEL` (**public**, owner-controlled; repository visibility does not grant a licence).

Host addresses, SSH private keys, and pinned known-hosts data remain in the selected GitHub Actions Environment. Never retrieve or print secret values, ask the owner to paste them into chat, or copy host addresses/credentials into repository files, issues, artifacts, shell output, or logs.

For the established Phase 0 LAB, the environment contract is documented in [PHASE0_TWO_HOST_LAB.md](PHASE0_TWO_HOST_LAB.md). For the dedicated Fleet Join LAB, use the separate contract in [FLEET_JOIN_LAB_ACCESS_PREFLIGHT.md](FLEET_JOIN_LAB_ACCESS_PREFLIGHT.md). Both use these secret names:
- `LAB_IRAN_HOST`
- `LAB_ABROAD_HOST`
- `LAB_SSH_PRIVATE_KEY`
- `LAB_SSH_KNOWN_HOSTS`

The SSH user/port and test-port settings are Environment variables. Do not assume that values or credentials from one Environment apply to the other.

## 3. Current Fleet Join LAB evidence and limits

The dedicated Fleet Join read-only preflight completed successfully in [run 44915734466](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44915734466), on `main` at `ec68e6d4f8960b601698deb139b5c62245ed3c40`. The result is recorded in the latest relevant [Issue #2 comment](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2#issuecomment-7064387237).

Observed inventory:
- Iran: Ubuntu 24.04.4 LTS.
- Abroad: Ubuntu 22.04.4 LTS; `nft` was not present.
- Iran-to-abroad TCP probes on 443, 8443, 2053, and 18300 reached the host but found no listener.

This was SSH/inventory/reachability evidence only. It did not inspect all managed paths, units, users, or Fleet CA state, and **did not prove either host disposable**. No package, service, filesystem, firewall, routing, sysctl, tunnel, or reboot changes were made. The OS-version mismatch must be resolved against the Ubuntu 22.04 acceptance baseline; do not install `nft` or change either host as an assumed fix. The missing `nft` binary is an inventory finding, not by itself proof that Fleet Join requires firewall tooling.

**No further host operation is authorized by this preflight result.** The Fleet Join acceptance plan requires freshly provisioned disposable Ubuntu 22.04 hosts (or an equivalently isolated systemd-capable environment), with a reviewed state boundary and separate task-specific owner authorization.

## 4. Supported read-only access paths

### Established Phase 0 LAB

Use [Phase 0 two-host LAB (manual, read-only preflight)](../.github/workflows/phase0-two-host-lab.yml) only when the task explicitly targets the established Phase 0 LAB:
1. Select branch `main`.
2. Enter exactly `READ_ONLY_PREFLIGHT` in the confirmation input.
3. Inspect both host reports and the Iran-to-abroad TCP probe results.

### Dedicated Fleet Join LAB

Use [Fleet Join LAB access preflight (manual, read-only)](../.github/workflows/fleet-join-lab-preflight.yml) only for the dedicated Fleet Join LAB:
1. Select branch `main`.
2. Enter exactly `FLEET_JOIN_READ_ONLY_PREFLIGHT` in the confirmation input.
3. Inspect both host reports and the Iran-to-abroad TCP probe results.

Both workflows use strict pinned host-key checking and ephemeral runner credential files with cleanup. Neither is a general-purpose remote shell. A successful preflight does not prove tunnel data flow, Fleet Join acceptance, or host disposability.

## 5. Before any additional real-host operation

Before any further LAB run:
- Re-read the latest Issue #2 comments and confirm no other LAB operation is active.
- Verify the exact task, host pair, branch/ref, candidate SHA, and required baseline.
- Announce the exact workflow/ref and scope on Issue #2 before execution.
- Obtain explicit, task-specific owner authorization.
- Use a dedicated narrowly scoped workflow with strict pinned host keys, masked host values, bounded timeouts, and unconditional ephemeral-key cleanup.
- Make state-changing work reversible; capture before/after evidence and define abort and cleanup criteria.
- Never upgrade an OS/systemd, reboot, alter firewall/routes/sysctl, or touch unrelated services as an implicit test step.

If the required isolated boundary or scoped workflow does not exist, report that precise blocker and create/review the missing workflow before attempting the operation. Never improvise direct access or expose credentials.

## 6. Required handoff after access

Append a concise comment to [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) recording:
- workflow run URL and exact commit/ref;
- which host checks completed (label hosts Iran/abroad; never disclose addresses);
- read-only vs state-changing scope;
- actual observed results and failures;
- confirmation that ephemeral credentials were cleaned;
- next executable action and any blocker.

Never claim an SSH connection, test, or cleanup succeeded unless the corresponding workflow step actually completed successfully.
