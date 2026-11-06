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
