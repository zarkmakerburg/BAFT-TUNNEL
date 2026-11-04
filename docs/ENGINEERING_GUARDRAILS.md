# Engineering Guardrails Learned from BAFT

**Binding process guidance for BAFT-TUNNEL.** Transfer lessons only. Do not transfer BAFT source code, Git history, credentials, keys, production configuration, CI secrets, or deployment state.

## Evidence and tests
1. Every result is tied to an exact commit SHA. Record branch, SHA, workflow run IDs and each result as PASS, FAIL, SKIP or NOT VERIFIED.
2. Missing status, queued jobs, timeouts, inaccessible logs and uncollected output are UNKNOWN, never PASS.
3. Preserve failed attempts. A rerun does not erase the original failure; explain what changed and why the later run is authoritative.
4. Reproduce defects RED before fixing them. Add a regression at the actual failure boundary.
5. A mock SSH test is not SSH connectivity. Loopback is not public reachability. A container network simulation is not real Iran-to-abroad E2E. Label every evidence level precisely.
6. Use observable readiness, not sleeps. Keep fixture-owned sockets alive until handoff. Avoid fixed-port test collisions and overlapping suites.
7. Keep platform limitations explicit. Linux/root-only failures on macOS remain failures or unverified locally; Ubuntu CI does not retroactively make local tests pass.

## Security and state
1. Fail closed on missing, malformed, stale or contradictory authority. Never convert corrupt security state or read errors into permissive defaults.
2. Atomic rename alone is not durability. For security state: restrictive temp file, complete write, fsync file, atomic rename, fsync parent directory. Define ordering for directory exchange and security-relevant deletion too.
3. Persist replay/anti-replay state durably before executing side effects. Process-crash tests do not prove power-loss durability.
4. Couple audit intent transactionally with security mutations. Do not mutate first and append best-effort later. Recover pending audit intents idempotently and report the true committed state on API errors.
5. Check every chmod, chown, fsync, rename, firewall, service-manager and cleanup error; verify resulting mode, owner, handle and state before advancing.
6. Authorization must hold at the effect boundary. A lease/policy check before slow inventory is insufficient. Serialize the final authority check with the actual OS/kernel mutation so expiry or policy drift cannot race.
7. Observed state, journals, desired reservations and cached verified markers are not authority after restart. Reconcile with fresh OS/kernel evidence and trusted authorization.
8. Track exact ownership. Remove only resources this installation created and can prove it owns; never infer ownership from port number or name.
9. Model partial apply, cleanup failure, crash, reboot, stale markers and uncertain outcomes explicitly. Recover safely or stop; never fabricate success.
10. Prove unrelated services and active traffic remain unchanged through add, update, rollback and removal.

## CI and supply chain
1. Pin third-party GitHub Actions to reviewed full commit SHAs, not mutable major tags.
2. Pin security scanners and build tools; never use @latest in security/release workflows.
3. Minimize workflow permissions per job. Do not expose long-lived signing secrets to repository-controlled code from a candidate tag.
4. Isolate release signing from candidate source code. Prefer offline or isolated signing over reviewed artifact digests.
5. Independently verify source SHA, signature chain, revocation state, checksums, archive contents and reproducible rebuild where feasible.
6. Add CI policy checks that reject mutable action refs, unpinned tools and signer-boundary regressions.

## Architecture, lifecycle and UX
1. CLI, API, agent and web wizard must use one validated lifecycle contract; do not create a UI-only shortcut around safe backend operations.
2. Adding a node/tunnel must be distinct from destructive re-pair or replacement.
3. Normal flow should be preflight → preview → explicit apply → verify → receipt. Advanced settings remain optional and explained.
4. Never report Ready from a control-plane heartbeat alone. Verify authenticated transport, listener ownership, destination reachability and real forwarded traffic using fresh evidence.
5. Treat shared transport as a capability to prove, not an assumption. Require negotiation, tenant isolation, limits and lifecycle tests; otherwise use a safe dedicated fallback.
6. Keep schema, runtime, docs, installer, API and UI in sync through contract tests.
7. Installer, update, backup, restore and uninstall are product features. Syntax checks alone do not prove them; test fresh install, rerun, interruption, reboot, rollback and restore on disposable hosts.

## Governance
1. Before editing, identify canonical branch, exact base SHA, branch owner and overlapping work. Never guess-merge divergent security-sensitive implementations.
2. Preserve other workers' branches, checkouts and stashes. No reset, force-push or overwrite of owned work.
3. Green CI does not override open P0s or unproven real-host acceptance. Keep PRs draft while gates remain.
4. Merge, tag, sign, publish, deploy and phase transition are separate authorization gates.
5. Use disposable LAB infrastructure. No production mutation during development without explicit owner approval.
6. Stop at the approved phase boundary and wait for owner approval before advancing.

## Mandatory change checklist
- Requirement and acceptance criterion identified.
- Canonical branch and exact base SHA verified.
- Baseline defect reproduced when applicable.
- Regression test added at the real boundary.
- Unit, integration, race, static and security tests run and recorded.
- Exact-SHA CI run IDs and outcomes recorded.
- Simulation, isolated-kernel, real-host and cross-border evidence distinguished.
- Existing resources and traffic proven unchanged where relevant.
- Failure, rollback, restart and recovery tested.
- Docs, schema, installer, API and UI kept consistent.
- Independent review for security-sensitive changes.
- No merge, release, deployment or phase transition without owner authorization.

## BAFT historical references
- Issue 67: HQ operating directives — https://github.com/zarkmakerburg/BAFT/issues/67
- Issue 72: security review findings — https://github.com/zarkmakerburg/BAFT/issues/72
- Issue 119: durability and supply-chain blockers — https://github.com/zarkmakerburg/BAFT/issues/119
- Issue 120: transactional audit outbox — https://github.com/zarkmakerburg/BAFT/issues/120
- Issue 269: P0 lifecycle and acceptance requirements — https://github.com/zarkmakerburg/BAFT/issues/269
- PR 270: ingress lifecycle and LAB history — https://github.com/zarkmakerburg/BAFT/pull/270
- PR 287: shared-listener tenant lifecycle limitations — https://github.com/zarkmakerburg/BAFT/pull/287

These are process references only. They do not authorize importing BAFT code or operational material.
