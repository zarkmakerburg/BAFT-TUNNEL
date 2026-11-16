# ADR-0035: update notice without an online agent or a root panel

Status: accepted (2026-11-16, owner's delegated decision authority). Roadmap: 0.3.0 O3. Builds on ADR-0025.

## Context

The panel should tell an operator that a newer signed stable release exists. Two obvious designs are wrong for this product:
an agent that calls the internet adds an outbound TLS client and a new failure and privacy surface to a loopback-only,
unprivileged process; a panel button that installs a release needs a root helper that downloads and runs code, which is a
larger trust-boundary change than this feature justifies.

## Decision

1. `scripts/update-check.sh` (operator-run, root or the agent's state owner) asks the GitHub "latest release" API for the newest
   stable tag, accepts only `vMAJOR.MINOR.PATCH`, and atomically writes `update-check.json` (`latest_tag`, `checked_unix`,
   `source`) into the agent state directory. It installs and changes nothing else. It is not scheduled by default; the menu
   and docs show how to run it or add a timer.
2. The agent stays offline. `GET /v1/update` (bearer-protected) only reads that file, validates it, and compares it with the
   running version. States: `unchecked` (no file), `current`, `update_available`, `unknown_installed`. A binary reporting `0.1.0`
   (every release up to v0.2.1; see PR #273) is `unknown_installed`: the notice never guesses.
3. The panel shows a notice with the latest tag and the signed one-line installer pinned to that tag (it verifies the Sigstore
   signature and checksums before running anything). The panel runs no command and holds no root. Install and update stay
   operator-run (`quick-install.sh`, `update.sh` with automatic rollback, ADR-0025).
4. A panel-triggered install is deferred until a privileged-helper ADR exists; it is not part of 0.3.0.

## Consequences

- The notice is only as fresh as the last `update-check.sh` run, and says when that was.
- No telemetry: the check is a single unauthenticated GET from the operator's host, initiated by the operator.
- Pre-release tags are never offered.

## Tests

`scripts/test-update-check.sh` (valid stable, prerelease refused, bad tag refused, atomic file mode) and agent unit tests for the
version comparison and every state.
