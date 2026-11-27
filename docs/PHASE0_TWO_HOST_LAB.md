# Phase 0 — two-host real-server LAB

The active Phase 0 LAB uses one owner-configured Iran host and one owner-configured abroad host. Host addresses are stored only in the private GitHub Actions Environment; they are intentionally not copied into repository documentation.

## Required GitHub Environment

Create/use the private Actions Environment named `baft-tunnel-lab`.

Variables:
- `LAB_IRAN_HOST`
- `LAB_IRAN_SSH_USER`
- `LAB_IRAN_SSH_PORT`
- `LAB_ABROAD_HOST`
- `LAB_ABROAD_SSH_USER`
- `LAB_ABROAD_SSH_PORT`
- `LAB_TEST_PORTS`

Secrets:
- `LAB_SSH_PRIVATE_KEY` — dedicated LAB SSH private key.
- `LAB_SSH_KNOWN_HOSTS` — independently verified, pinned ED25519 host keys for both hosts.

Never put private keys, host-key material, or host addresses in repository files, workflow YAML, issues, or logs. Do not paste private keys into chat.

## Run the read-only preflight

1. Open **Actions → Phase 0 two-host LAB (manual, read-only preflight)**.
2. Select the `main` branch (the same workflow is present on the Phase 0 branch).
3. Select **Run workflow** and enter exactly `READ_ONLY_PREFLIGHT`.
4. Review both host reports and the Iran-to-abroad TCP port results.

The workflow is `.github/workflows/phase0-two-host-lab.yml`. It uses strict host-key verification and temporary runner credentials, removed in an `always()` cleanup step.

## Re-pin host keys after an address change

`LAB_IRAN_HOST` / `LAB_ABROAD_HOST` and `LAB_SSH_KNOWN_HOSTS` are independent secrets. Changing only the addresses leaves the pinned `known_hosts` pointing at the old ones, and every real-host workflow then fails closed at the first SSH call with `No ED25519 host key is known for … and you have requested strict checking`. That is the intended behaviour; do not add `accept-new` or relax `StrictHostKeyChecking` to get past it.

Re-pin with the operator-run helper (local machine, never CI):

1. `lab/real-hosts/re-pin-host-keys.sh --iran <iran-address> --abroad <abroad-address> --expect-iran SHA256:… --expect-abroad SHA256:…`
2. Copy both fingerprints from the provider console for the independent check. A mismatch means the address now resolves to a different machine; stop and investigate.
3. Paste the produced (mode `0600`) file into the `baft-tunnel-lab` Environment secret `LAB_SSH_KNOWN_HOSTS`.
4. Re-run the read-only preflight.

The helper prints fingerprints only; the known_hosts lines are written to a private file and printed only with an explicit `--print-known-hosts`. Its contract test is `lab/real-hosts/test-re-pin-host-keys.sh` (real `ssh-keygen` key material, `ssh-keyscan` mocked, no network).

## Safety and evidence limits

The preflight reads OS/kernel/architecture/tool availability and performs TCP connection probes from Iran to abroad on configured ports. It does not install software, write persistent remote files, alter firewall/routing, restart services, or reboot either host.

The original real-host preflight is recorded in [GitHub Actions run 42221903167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42221903167). Both hosts reported Ubuntu 22.04.x and the expected baseline tools. Probes on ports 443, 8443, 2053 and 18300 reported host-reachable/no-listener. This proves only read-only SSH inspection and observed TCP reachability at that time; it does not prove a tunnel listener or application traffic.

The later Phase 1 Direct/Reverse tunnel test is separate evidence and is recorded in the HQ board and PR #3. Do not describe a read-only preflight as tunnel E2E.

No firewall, route, sysctl, systemd service, persistent file or reboot operation is authorized by this workflow.
