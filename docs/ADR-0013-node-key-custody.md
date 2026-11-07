# ADR-0013: Node private-key custody

- Status: Accepted; runtime resolver integrated; ephemeral systemd 255 encrypted-delivery, restart/redelivery, and installed-service lifecycle CI PASS; dedicated operator-host custody proof pending
- Scope: Linux systemd deployments in Phase 1
- Date: 2026-11-06

## Context

The TLS runtime currently loads an unencrypted PEM node private key from a filesystem path. Restrictive ownership and mode (root:baft-tunnel, 0640, under a 0750 directory for the systemd copy) reduce local disclosure, but do not provide encryption at rest. The Rustls configuration path consumes private-key material in process memory; a file-permission change alone cannot provide hardware-backed signing or eliminate in-memory key exposure.

The current LAB hosts run Ubuntu 22.04/systemd 249. Standard LoadCredential= is the supported baseline service-credential mechanism; LoadCredentialEncrypted= and systemd-creds are optional on systemd 250+. See ADR-0016 for the compatibility policy.

## Decision

1. Keep the current PEM-file mode explicitly classified as a development/LAB compatibility mode. It must never be described as encrypted-at-rest custody or production-grade protection.
2. Add an opt-in systemd credential source for node keys. The service manager, not BAFT-TUNNEL, decrypts the credential before process start and exposes it through $CREDENTIALS_DIRECTORY. The application resolves only a strict credential identifier; it must not accept arbitrary environment-expanded paths.
3. For baseline deployments (systemd 247+), use LoadCredential= with a root:root 0600 source file in a root:root 0700 directory. This is access-controlled plaintext at rest, not encrypted storage. For optional encrypted-at-rest deployments, require systemd 250+ and LoadCredentialEncrypted=. Prefer TPM2-bound encryption where supported; otherwise use systemd's host credential key with its documented host/OS binding limitations. Provisioning must fail closed if the selected encryption capability is unavailable.
4. Do not silently fall back from encrypted credentials to plaintext LoadCredential= or PEM files. A deliberate operator-selected LAB compatibility mode is separate and visibly reported.
5. Keep credential plaintext out of JSON configuration, command-line arguments, environment variable values, logs, plans, and support bundles. Only the credential identifier may appear in configuration.
6. Treat the credential directory as service-scoped, read-only input. Validate the resolved file as a regular file, reject symlinks where the platform permits reliable detection, and enforce a conservative size bound before parsing.
7. The current Rustls API receives private-key bytes in process memory. This design improves at-rest protection and delivery isolation; it does not claim TPM-backed non-exportable signing. A PKCS#11/TPM signer integration is a separate future architecture decision.

## Runtime integration

The runtime JSON accepts exactly one private-key source:
- key: a filesystem path retained for explicit LAB/development compatibility.
- key_credential: a strict systemd credential identifier resolved only beneath CREDENTIALS_DIRECTORY. On systemd 249+, the unit may deliver it through LoadCredential=; systemd 250+ may optionally use LoadCredentialEncrypted=.

The two fields are mutually exclusive. The resolver rejects missing credential environment, invalid identifiers, absent files, symlinks, non-regular files, empty/oversized files, and canonical paths outside the credential directory. config validate and apply both resolve the credential source before TLS key parsing; there is no fallback to key when key_credential is selected. Plans and support bundles expose the source class and credential identifier, never the filesystem path or key bytes.

Encrypted delivery is configured per service instance through a systemd drop-in, not in the shared unit. This preserves compatibility with LAB/file-mode instances and prevents a credential dependency from being imposed on every instance.

## Compatibility and deployment gate

- Ubuntu 22.04 / systemd 249: supported baseline using LoadCredential= and root-only source storage. This does not encrypt the source key at rest.
- Ubuntu 24.04 / systemd 255 and other systemd 250+ hosts: may optionally use LoadCredentialEncrypted=, subject to capability checks and host-specific TPM/host-key provisioning.
- A unit must not declare an unconditional credential dependency that breaks existing file-mode LAB instances. Encrypted credentials are enabled only by an explicit per-instance drop-in or equivalent generated unit configuration.
- Never copy the offline CA private key to a tunnel host.

## Security and acceptance tests

- Missing CREDENTIALS_DIRECTORY, missing credential ID, invalid ID, absent file, symlink, non-regular file, oversized key, and malformed PEM all fail closed.
- Credential identifiers cannot escape the credential directory or introduce path separators.
- Support bundles and plans expose only the selected key-source class and credential ID, never contents or passphrases.
- Unit-level tests cover resolution and rejection cases; Linux integration tests verify systemd credential delivery, permissions, restart behavior, and encrypted credential decryption.
- Hosted Ubuntu 24.04/systemd 255 CI demonstrates encrypted credential delivery through a real transient systemd unit, successful BAFT runtime TLS-key parsing, and systemd restart/redelivery after a controlled first-run failure. A separate disposable Ubuntu 24.04/systemd 255 runner test installs the actual BAFT binary and template, configures LoadCredentialEncrypted=, verifies service identity, performs start/restart/stop, and safely uninstalls; output BAFT_SYSTEMD_SERVICE_LIFECYCLE=PASS. These ephemeral tests do not establish persistent operator-host custody, TPM protection, or encrypted host storage. A dedicated operator-controlled Linux host must still demonstrate encrypted credential mode, permissions, restart behavior, upgrade/rollback and uninstall before the production key-custody gate can close.
- No plaintext fallback is permitted when encrypted mode is selected.

## Consequences

The encrypted-credential delivery path and installed service lifecycle are exercised in ephemeral hosted CI on Ubuntu 24.04/systemd 255, including the actual runtime key parser and service start/restart/stop/uninstall. This is not a completed production custody feature: the runner is disposable and systemd warned that its host credential secret file was not on encrypted media. The existing two Ubuntu 22.04 LAB hosts remain suitable for transport and compatibility testing, but cannot exercise the encrypted systemd credential path. Persistent operator-host lifecycle and dedicated-host proof remain open. The Ubuntu 22.04 baseline remains supported with standard LoadCredential=; operators requiring encrypted-at-rest credential files may choose the optional systemd 250+ mode or a separately reviewed hardware-backed signing provider.
