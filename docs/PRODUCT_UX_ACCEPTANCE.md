# Product UX and Installer Acceptance Criteria

**Status:** Phase 0 requirements; not an assertion that these features are implemented.

## Product promise
An operator should be able to install the product, pair two nodes, define a forwarded service, and verify real traffic without learning the internal transport architecture. Advanced settings must not be required for the normal path.

## First-run journey
1. Preflight checks OS/architecture, privileges, outbound connectivity, ports, disk, clock, and conflicting listeners without changing the host.
2. Install a signed, checksum-verified package and create a dedicated service identity. Re-running is safe.
3. Pair the second node with a short-lived, single-use enrollment code. Clearly show which side initiates outbound connectivity.
4. Create a tunnel through a small wizard: source, destination, protocol/service, and ports.
5. Preview changes, port conflicts, firewall impact, and rollback before apply.
6. Verify a fresh authenticated handshake and actual forwarded bytes.

## Acceptance criteria
- Normal setup requires no manual configuration-file editing.
- Every step is retryable and partial installation can be diagnosed and rolled back without deleting unrelated resources.
- No destructive apply occurs before review and explicit confirmation.
- A tunnel is never marked healthy based only on a control-plane heartbeat.
- Failure messages identify the failing layer and provide copyable diagnostics without secrets.
- Uninstall removes only resources created and recorded by that installation.
- Persian RTL is the default; English is available. Mobile and keyboard workflows remain usable.
- E2E tests cover fresh install, retry, port conflict, interruption, rollback, re-enrollment, and uninstall.

## Status vocabulary
Control plane, authenticated transport, listener, and data plane have separate states. Missing or stale evidence is unknown/stale, never healthy.
