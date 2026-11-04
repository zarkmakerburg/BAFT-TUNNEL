# Real-host LAB — 2 Iran + 4 abroad nodes

BAFT-TUNNEL's independent LAB is designed for six owner-provided test servers:

| Role | GitHub Actions environment variable | Purpose |
|---|---|---|
| Iran 1 | `IR_1_HOST` | Iran-side node 1 |
| Iran 2 | `IR_2_HOST` | Iran-side node 2 |
| Abroad 1 | `EX_1_HOST` | Abroad node 1 |
| Abroad 2 | `EX_2_HOST` | Abroad node 2 |
| Abroad 3 | `EX_3_HOST` | Abroad node 3 |
| Abroad 4 | `EX_4_HOST` | Abroad node 4 |

The preflight inspects all six nodes and checks the complete 2 × 4 Iran-to-abroad TCP reachability matrix (eight directed paths). It distinguishes open ports, reachable hosts without listeners, timeouts, and other network errors.

## Configure GitHub

Create a GitHub Actions **Environment** named `baft-tunnel-lab`. Add these **Variables** to that environment:

- `IR_1_HOST`, `IR_2_HOST`, `EX_1_HOST`, `EX_2_HOST`, `EX_3_HOST`, `EX_4_HOST`
- `LAB_SSH_USER` (optional; defaults to `bafttunnel-lab`)
- `LAB_SSH_PORT` (optional; defaults to `22`)
- `LAB_TEST_PORTS` (optional; defaults to `443,8443,2053,18300`)

Add these **Secrets** to the same environment:

- `LAB_SSH_PRIVATE_KEY` — dedicated BAFT-TUNNEL LAB private key authorized on all six test hosts.
- `LAB_SSH_KNOWN_HOSTS` — pinned host-key entries for all six hosts, populated from independently verified fingerprints.

If `LAB_SSH_PORT` is not 22, entries must use OpenSSH `[host]:port` notation for each affected host. Do not trust `ssh-keyscan` output without independent out-of-band fingerprint verification. Prefer a dedicated low-privilege account and key; never reuse personal or BAFT credentials.

Never put private keys or host-key material in repository files, Actions Variables, workflow YAML, issues, or logs.

## Local configuration

Copy `lab/real-hosts/.env.example` to `lab/real-hosts/.env`, fill the six TEST-host addresses and credential file paths, then run:

```bash
bash lab/real-hosts/run-local-preflight.sh
```

The `.env` file is git-ignored. It must contain only test-host addresses and local credential file paths, never private key contents. Keep the private key and `known_hosts` outside the repository with restrictive permissions.

## Run the safe preflight

1. Configure the environment variables and secrets above.
2. Open **Actions → Real-host LAB (manual, read-only preflight)**.
3. Select **Run workflow** and enter exactly `READ_ONLY_PREFLIGHT`.
4. Inspect the six host reports and eight Iran-to-abroad path results.

The job uses the `baft-tunnel-lab` GitHub Environment, where environment protection rules can be enabled. The real-host job only runs on manual dispatch with the exact confirmation string. Pushes and pull requests run mock-only tests and never connect to servers.

## Local mock tests

```bash
bash lab/real-hosts/test-preflight-matrix.sh
```

The test uses a fake `ssh` executable and documentation-only TEST-NET IP addresses. It makes no network connection. It verifies all six role inputs, duplicate-host rejection, port validation, strict host-key checking, and the expected 14 SSH calls (six host inspections plus eight path probes).

## Requirements and safety boundaries

- Runner requirements: Bash, OpenSSH client, Python 3. Remote hosts must provide Bash and Python 3; basic Linux tools are inspected and reported.
- This is a **read-only preflight**, not an installer or active tunnel test. It does not install packages, write remote files, change firewall rules, stop/restart services, or reboot hosts.
- Do not use production/customer hosts. The owner must ensure all six configured hosts are dedicated test infrastructure and that the dedicated account has only the permissions needed for inspection.
- Do not run active tests until a separate, reviewed active-test plan defines isolated service names, ports, firewall ownership, rollback, collision detection, and cleanup proof. This workflow intentionally has no active-test switch.
- Docker/netem results are simulation evidence. Six-host preflight proves SSH access and observed TCP reachability only. It does not prove tunnel establishment, throughput, failover, censorship resistance, or production readiness.
- Avoid sharing host addresses in Actions logs or issues. The workflow masks the six configured host values before running preflight.
