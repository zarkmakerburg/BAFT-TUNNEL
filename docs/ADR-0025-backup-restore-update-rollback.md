# ADR-0025: backup/restore and guarded update with automatic rollback

Status: accepted (2026-11-09, under the owner's delegated decision authority).
Builds on ADR-0013 (key custody), ADR-0015 (CRL activation) and the existing `install.sh` / `rollback.sh`.
Roadmap: P6 "backup/restore, update lifecycle"; alignment document §6 item 3.

## Context

`install.sh` already backed up the binary, unit and one instance credential, and `rollback.sh` restored them. Nothing backed up
or restored the rest of a node (config, PKI, CRL, credentials, drop-ins, state), and an update was a manual install, restart
and hope: a build that starts but cannot run left the tunnel down until an operator noticed.

## Decision

1. `scripts/backup.sh` writes `/var/backups/baft-tunnel-backup.XXXXXX` (root, 0700) with `backup.tar` (0600), a SHA-256 file and
   a manifest of names and modes only. Scope: `/etc/baft-tunnel` (config, PKI, CRL, credentials), `baft-tunnel@*.service.d`
   drop-ins, `/var/lib/baft-tunnel`. The binary is excluded (install.sh keeps binary backups). Output never contains key material.
   The archive does hold private keys, so the directory stays root-only and the docs say to delete it when no longer needed.
2. `scripts/restore.sh --yes BACKUP_DIR` verifies the checksum, then an allowlist of paths and entry types (regular files and
   directories only; no links, devices or `..`), refuses while instances run unless `--stop`, takes a pre-restore snapshot of
   the current state, replaces each tree by rename with automatic undo on failure, forces credentials to root-only modes, runs
   `daemon-reload` and starts nothing. A restore is accepted only when the service actually comes back (see tests), not on archive success.
3. `scripts/update.sh --binary NEW` = data backup, `install.sh` (keeps the previous binary), restart of the instances that
   were active, health check, and on any failure `rollback.sh`, restart and re-verify. Health: each instance stays active for
   `--health-secs` (default 10, longer than the unit's `RestartSec=3s`) with no automatic restart, plus an optional
   `--health-cmd`. Exit 0 updated, 1 rejected and rolled back and verified healthy, 3 rollback unhealthy or config digest changed.
   A SHA-256 digest of `/etc/baft-tunnel` before and after proves config and keys were not touched. Stopped instances stay stopped.
4. No new transport, engine, network surface or daemon. Update is operator-run and local; remote Job-driven update waits on the
   enrollment ADR.

## Consequences and limits

- Rollback restores the previous binary and unit only. It does not undo config or key changes, which update never makes.
- The health check proves "process stays up", not "tunnel carries data"; run the Phase 1 E2E for that. The binary-only
  scope of the automatic rollback is deliberate.
- Backups from install.sh and this ADR are separate and both remain root-only.

## Tests

- `scripts/test-backup-update.sh` (phase0 `lab-config`): permissions, checksum, no secrets in output, byte-identical restore,
  tampered/link/outside-path archives refused, successful update, failed health check rolled back, refusals change nothing.
- `scripts/pki/test-systemd-baseline-lifecycle.sh` (phase1 `ubuntu-2204-baseline`, Ubuntu 22.04 / systemd 249): real
  guarded update, a build that passes `--version` but crashes is rolled back automatically with the old version serving
  again, config/keys digest unchanged, no private key material in logs or journal, tampered restore refused without stopping
  the service, destroyed config/PKI/key restored and the service restarted.
