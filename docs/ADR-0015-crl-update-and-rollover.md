# ADR-0015: offline CRL update, activation, and rollback

- **Status:** Accepted for Phase 1 operator-controlled CRL lifecycle
- **Date:** 2026-11-07
- **Scope:** Existing offline-CA / two-node TLS deployment; no online control plane

## Context

Both TLS peers load a CA-signed PEM CRL during process startup. Rustls enforces revocation and CRL validity and fails closed when status is unknown. The current operator procedure copies a new CRL out of band and restarts both peers manually, without a guarded node-side activation or rollback helper. A partially completed rollout can interrupt the tunnel, and a stale CRL could accidentally restore a revoked certificate if an operator overwrites the current file.

The CA private key must remain offline. The two nodes must not gain authority to issue CRLs, contact a signer, or silently fetch trust material. Online enrollment, remote distribution, and a network-accessible signer remain out of scope.

## Decision

1. **Offline issuance remains authoritative.** The operator generates the updated CRL on the offline CA. Only the signed CRL is transferred to each node. The CA key, CA database, passphrase, and signing operation never leave the offline environment.
2. **Node-side activation is explicit and local.** Provide a root-only helper that accepts the trusted CA certificate, current CRL path, candidate CRL path, and one explicitly named BAFT-TUNNEL systemd instance. It does not discover peers, open network connections, modify firewall rules, or enable services.
3. **Fail closed before mutation.** Before replacing anything, verify the candidate CRL signature against the configured CA, issuer binding, presence of a CRL Number, validity window, and monotonic increase over the currently installed CRL. Reject malformed, expired, not-yet-valid, unsigned, wrong-issuer, equal-number, or lower-number candidates. The existing CRL must itself verify and have a CRL Number; no implicit bootstrap or missing-current-CRL fallback is allowed.
4. **Atomic local activation.** Serialize cooperating updates with a per-CRL flock. Stage the candidate in the same directory, revalidate the exact staged bytes under the lock, preserve a root-owned readable CRL mode, and atomically rename it over the live file. Keep a root-only (0600) temporary backup until the selected service has restarted and is active.
5. **Bounded rollback.** If restart or active-state verification fails, atomically restore the prior CRL and attempt to restart the same service against the restored CRL. Report both the activation failure and any rollback failure. Never claim success if the service is not active.
6. **No automatic two-node transaction.** There is no distributed atomic commit across two independent systemd hosts. The operator must stage the same candidate on both nodes, verify the same CRL number/fingerprint out of band, then activate each node deliberately. If either activation fails, stop the rollout and restore the prior CRL on any node already updated. A revoked peer may be rejected as soon as one side activates; temporary tunnel interruption is expected and fail-closed.
7. **No automatic CA or pin rollover.** CRL updates do not rotate the trust anchor or leaf pins. CA rollover and overlapping-pin transitions require a separate reviewed design with explicit overlap, expiry, rollback, and compromise procedures.
8. **Auditability without secrets.** Print the CRL number, SHA-256 fingerprint, target instance, and result. Never print private-key material or attempt to access the CA private key.

## Operational sequence

1. On the offline CA, revoke the intended certificate and generate a new CRL using the existing PKI helper.
2. Verify the CRL and its number on the offline CA; transfer only the CRL to both nodes over the operator's approved channel.
3. On both nodes, validate the candidate against the same CA certificate and confirm identical CRL number and fingerprint before activation.
4. Activate one selected instance at a time using the node helper. Confirm `systemctl is-active` before proceeding.
5. Verify the revoked identity is rejected and the intended non-revoked peer reconnects. If any step fails, restore the prior CRL on all nodes already updated and verify service recovery.

## Consequences and limitations

- This closes the unsafe local file-replacement/restart gap, not remote distribution or a distributed transaction.
- The operator remains responsible for secure CRL transfer, matching candidate artifacts across nodes, sequencing, and post-activation peer verification.
- Rollback restores the prior trust state and may re-admit a certificate revoked by the candidate; it is an availability recovery mechanism, not a security-neutral action. Use only under an explicit incident decision.
- CRL Number monotonicity prevents accidental stale-file rollback within one CA lifecycle. It does not solve CA key compromise, CA rollover, or pin rollover.
- The helper requires Linux, root, OpenSSL, Python 3, and systemd. It must not be run against the existing Ubuntu 22.04 LAB hosts as a persistent service change without a separately approved LAB operation.
