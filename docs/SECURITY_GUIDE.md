# Security Guide

## Phase 0 rules
- Use only dedicated test infrastructure for LAB work.
- Do not copy BAFT credentials, SSH keys, host-key records, release keys, or production configuration.
- Keep private keys outside the repository. Use GitHub Actions Environment secrets for remote-host tests.
- Pin SSH host keys from independently verified fingerprints; never disable host-key checking.
- Use a dedicated low-privilege SSH account. The current preflight requires read-only inspection only.
- Do not run active install, firewall, service, reboot, or cleanup operations in Phase 0.
- Never log forwarded payloads, private keys, enrollment tokens, or complete environment dumps.

## Repository controls
The canonical repository is private and has no public open-source license. Grant access only to explicitly approved collaborators. Keep Actions logs and artifacts restricted. Rotate credentials immediately if they are exposed.

## Before active host tests
A separate review must define isolated service names and ports, firewall ownership, collision detection, backups, rollback, cleanup proof, and an explicit owner approval gate.

## Phase 1 offline PKI handling

- Keep the encrypted CA private key on an offline administration machine. Never copy `ca.key` to either tunnel endpoint or into this repository.
- Generate each node's EC P-256 private key on that node with `scripts/pki/baft-pki.sh new-node`; transfer only the CSR to the offline CA.
- The enrollment key remains mode `0600`. For the current systemd unit, deploy a separate copy as root:`baft-tunnel` mode `0640` under a root:`baft-tunnel` mode `0750` parent directory. This lets only root and the dedicated service group read the key; do not make the enrollment workspace or key world/group-readable.
- Node private keys are unencrypted PEM files. Restrictive ownership and permissions are a temporary custody baseline, not encrypted-at-rest key storage or HSM integration.
- The CA issues only end-entity certificates with the exact node URI SAN and the operator-selected DNS/IP SAN used by `--server-name`. The CSR cannot inject extensions; the CA supplies both SANs.
- Revocation is enforced by a CA-signed CRL on both TLS peers. Distribute the updated CRL to both peers and restart both processes. CRL publication/distribution is manual.
- Leaf-certificate pinning means certificate rotation requires a coordinated config update on both peers. Overlapping pin sets and automated rollover are not implemented.
- Never use a test CA, node key, or test certificate on production systems.
