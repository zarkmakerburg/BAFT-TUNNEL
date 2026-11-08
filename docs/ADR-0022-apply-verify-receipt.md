# ADR-0022: Apply, Verify, Receipt

Status: accepted (owner priority 3, 2026-11-08; owner delegated decisions).
Builds on ADR-0011 (authorization), ADR-0020 (managed runtime host), ADR-0021 (topology and Reverse).

## Decision

1. **Scope.** The agent can apply exactly the two local, file-level stages the CLI already has:
   `init` (node key or PKI check, pairing state, public bundle) and `join` (verify the peer bundle,
   write the final configuration). It never starts or stops a service, changes a firewall, or
   contacts the peer. Starting the tunnel remains an operator command, listed in the receipt.
2. **Endpoint.** `POST /v1/setup/apply` (bearer token, state directory required):
   `{stage, confirm: true, idempotency_key, plan{...}, bundle?, checksum?}`. `confirm` must be the
   literal `true`. The key is 8-64 characters of `A-Za-z0-9-_`.
3. **Gate.** `init` re-runs the preflight on the agent for the same plan. Any `blocked` check about
   this host refuses the apply with HTTP 409 and a `refused` result; nothing is written. Peer-scope
   checks (reachability, identity, firewall) are reported but do not gate a local init. `join` is
   gated by the setup code itself (state present, checksum, matching identities, no overwrite).
   Capability gating is part of the preflight, so Noise Reverse and BAFT cannot be applied.
4. **Verify.** After the stage, the result is checked from the files: state parses and names this
   node, bundle checksum equals the reported one, key is not group/world accessible and not
   present in the bundle (init); the configuration loads, validates, matches the engine, is not
   world-accessible and the stored peer bundle is byte-identical (join).
5. **Rollback.** If the stage or any check fails, only the files this apply created are removed;
   pre-existing files are never touched. The receipt records `rolled_back` with the removed paths.
6. **Receipt.** A JSON document stored once per idempotency key in `<state-dir>/receipts/<key>.json`
   (directory 0700, file 0640) and an audit line (`action: apply`). It lists request, gate result,
   changes (path, kind, mode, SHA-256 for public files only), verification, rollback, `not_done`
   and next commands. It contains no key material, token or bundle content. The same key with the
   same request returns the stored receipt (`replayed: true`); the same key with a different
   request is a 409.
7. **Concurrency.** One apply at a time per agent.
8. **UI.** Wizard step 6 shows an explicit confirm checkbox and an "Apply and verify" button
   (disabled while preflight is blocked) and renders the receipt; the copyable commands stay.

## Not covered (stated, not faked)

- Service start/enable, firewall, peer-side verification and end-to-end data-path verification.
- Rollback of anything the apply did not create.
- Applying Reverse with Noise, or any BAFT engine.

## Evidence

Unit tests (`apply::tests`, `agent::tests::apply_*`), browser QA (`scripts/panel-qa/browser-flows.js`)
and a live agent run that produced a verified receipt on a temporary directory. No LAB run was used.
