# Pairing LAB on the dedicated Fleet hosts — staged authorization plan

**Status:** Owner authorized preparation of an isolated Pairing path on 2026-11-22. This document does not authorize a Pairing run, remote cleanup, installation, enrollment, tunnel startup, or any other host mutation.

**Only hosts/environment:** the two already-registered hosts in GitHub Environment `baft-tunnel-fleet-lab-20261121`:
- Iran / Hub: observed Ubuntu 24.04.4 LTS.
- Abroad / Member: observed Ubuntu 22.04.4 LTS.

Do not use `baft-tunnel-lab`, provision replacement VMs, reimage either host, or change the Ubuntu support baseline. Keep Fleet Join and Pairing workflows separate. Both must share concurrency group `baft-tunnel-trusted-lab-hosts`.

## Why the existing Pairing workflow is not reusable as-is

The current `.github/workflows/phase1-trusted-pairing-lab.yml` targets `baft-tunnel-lab` and runs `lab/real-hosts/clean-test-host.sh` before the Pairing stage. That cleanup script requires Ubuntu 22.04 and performs host cleanup. It must not be pointed at the mixed-version Fleet hosts.

The Pairing stage itself has useful fail-closed checks: it refuses pre-existing BAFT-TUNNEL paths, units, accounts, state and backups before installation, then records run ownership and invokes a scoped host cleanup helper for resources created by that run. The new Fleet path must preserve those checks and must never run the generic clean-host script.

## Stage 0 — read-only inventory (prepared; not yet run)

The new manual workflow `pairing-fleet-read-only-inventory.yml` uses only the dedicated Fleet Environment and the existing read-only inventory script. It:
- requires a typed confirmation and dispatch from `main`;
- shares the real-host concurrency lock;
- uses pinned SSH host keys with strict checking and disables the system-wide known-hosts file;
- inventories OS/kernel, BAFT-TUNNEL unit/process names, selected path metadata, listener addresses, and package matches;
- does not create remote files, inspect secret contents, stop services, clean files, install packages, or change networking.

A successful SSH match proves only that the configured known_hosts pin was accepted. Provider-console/API fingerprint cross-check remains outstanding and must be independently verified before mutation.

The current inventory script is a first-pass inventory, not a full safety attestation. It does not establish host disposability, reset/recovery boundaries, ownership of every BAFT-looking artifact, complete user/group membership, firewall/routing state, or the safety of removing any existing resource. Treat missing information as UNKNOWN.

## Stage 1 — classify and bound any pre-existing state

After Stage 0, classify each discovered item by exact path/unit/process/package, owner, purpose, and whether it belongs exclusively to a previous BAFT-TUNNEL Pairing run. Preserve anything shared, ambiguous, Fleet Join-owned, or not proven run-owned.

If any pre-existing BAFT-TUNNEL resource exists, stop before Pairing. Produce an exact proposed cleanup manifest (resource, host, evidence of ownership, proposed operation, expected postcondition). Obtain owner confirmation of that exact manifest before cleanup. No blanket `clean-test-host.sh`, recursive wildcard deletion, service stop, or broad account removal.

If there are no pre-existing BAFT-TUNNEL resources, record the clean baseline and proceed to the next gate without cleanup.

## Stage 2 — independent Pairing workflow design

A separate Pairing workflow may be added only with these constraints:
- Environment is exactly `baft-tunnel-fleet-lab-20261121`; never the old Pairing environment.
- Do not invoke `clean-test-host.sh`.
- Keep the exact signed candidate verification and immutable SHA pin from the current trusted Pairing workflow; do not build an unreviewed binary on the runner.
- Run only after Stage 0 identity/inventory gates and any exact cleanup manifest have been accepted.
- Preserve the Pairing stage's pre-existing-resource refusal, run-owned markers, per-instance cleanup manifests, and fail-closed preservation on mismatch.
- Use the shared real-host concurrency group.
- No `CAP_NET_ADMIN`, TUN, route, firewall, sysctl, reboot, package-manager mutation outside the reviewed installer, or unrelated service changes.
- Preserve evidence and host state on ambiguous failures; never broaden cleanup to make a run pass.

## Stage 3 — execution gate

A separate explicit owner authorization is required after review of:
1. provider-console host-key identity cross-check;
2. full read-only inventory and exact baseline;
3. runtime compatibility of the same signed candidate on Ubuntu 24.04 and 22.04;
4. local tests proving the Fleet workflow cannot invoke generic cleanup or access the old Pairing Environment;
5. exact host-side mutation manifest and rollback boundary.

Only then may the dedicated Fleet Pairing workflow be dispatched. This approval does not imply Ubuntu 24.04 is generally supported or that Fleet Join is implemented.

## Evidence labels

- Existing run `44948802945`: SSH access and basic host facts only; not a Pairing acceptance test.
- New inventory workflow: read-only inventory only; no host mutation.
- Pairing PASS requires a later exact-candidate, two-host run with verified post-state and all acceptance assertions. Never infer PASS from workflow preparation or SSH reachability.
