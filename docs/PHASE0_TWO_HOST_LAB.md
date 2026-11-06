# Phase 0 — two-host real-server LAB

This Phase 0 workflow targets the currently provisioned pair only:

| Role | Environment variable | Host |
|---|---|---|
| Iran | `LAB_IRAN_HOST` | `193.201.66.98` |
| Abroad | `LAB_ABROAD_HOST` | `104.253.134.9` |

The addresses are configured in the private GitHub Actions Environment `baft-tunnel-lab`, not hard-coded into workflow files.

## Required Environment variables

- `LAB_IRAN_HOST`
- `LAB_IRAN_SSH_USER`
- `LAB_IRAN_SSH_PORT`
- `LAB_ABROAD_HOST`
- `LAB_ABROAD_SSH_USER`
- `LAB_ABROAD_SSH_PORT`
- `LAB_TEST_PORTS`

## Required Environment secrets

- `LAB_SSH_PRIVATE_KEY` — dedicated private key authorized for the two test hosts.
- `LAB_SSH_KNOWN_HOSTS` — pinned ED25519 host keys for both hosts, independently checked against provider-console fingerprints.

Do not place private keys or host-key material in repository files, variables, issues, or logs. The private key must never be pasted into chat.

## Run

1. Open **Actions → Phase 0 two-host LAB (manual, read-only preflight)**.
2. Select branch `phase0/two-host-lab-preflight`.
3. Select **Run workflow** and enter exactly `READ_ONLY_PREFLIGHT`.
4. Review both host reports and the Iran-to-abroad TCP port results.

## Safety and evidence limits

This workflow only uses SSH to read OS/kernel/architecture/init/tool availability and performs TCP connection probes from Iran to abroad on the configured ports. It does not install software, write remote files, alter firewall/routing, restart services, or reboot either host.

A successful preflight proves only SSH reachability, basic host facts, and observed TCP connectivity at test time. It does not prove tunnel functionality, throughput, resilience, censorship resistance, or production readiness. No real-host run is considered complete until its GitHub Actions result is inspected.
