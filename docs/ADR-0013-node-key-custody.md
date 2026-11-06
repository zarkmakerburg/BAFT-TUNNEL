# ADR-0013: Node private-key custody

- Status: Accepted; runtime resolver integrated; transient systemd 255 encrypted-delivery CI integration PASS; dedicated-host lifecycle proof pending
- Scope: Linux systemd deployments in Phase 1
- Date: 2026-11-06

## Context

The TLS runtime currently loads an unencrypted PEM node private key from a filesystem path. Restrictive ownership and mode (root:baft-tunnel, 0640, under a 0750 directory for the systemd copy) reduce local disclosure, but do not provide encryption at rest. The Rustls configuration path consumes private-key material in process memory; a file-permission change alone cannot provide hardware-backed signing or eliminate in-memory key exposure.

The current LAB hosts run Ubuntu 22.04. The systemd service-credential encryption mechanism (LoadCredentialEncrypted= and systemd-creds) was added in systemd 250. Ubuntu 22.04's systemd 249 baseline therefore cannot be assumed to support it. The feature must be capability-detected, not inferred from the distribution name.

## Decision

1. Keep the current PEM-file mode explicitly classified as a development/LAB compatibility mode. It must never be described as encrypted-at-rest custody or production-grade protection.
2. Add an opt-in systemd credential source for node keys. The service manager, not BAFT-TUNNEL, decrypts the credential before process start and exposes it through $CREDENTIALS_DIRECTORY. The application resolves only a strict credential identifier; it must not accept arbitrary environment-expanded paths.
3. For encrypted-at-rest deployments, require systemd 250+ and LoadCredentialEncrypted=. Prefer TPM2-bound encryption where supported; otherwise use systemd's host credential key with its documented host/OS binding limitations. Provisioning must fail closed if the selected encryption capability is unavailable.
4. Do not silently fall back from encrypted credentials to plaintext LoadCredential= or PEM files. A deliberate operator-selected LAB compatibility mode is separate and visibly reported.
5. Keep credential plaintext out of JSON configuration, command-line arguments, environment variable values, logs, plans, and support bundles. Only the credential identifier may appear in configuration.
6. Treat the credential directory as service-scoped, read-only input. Validate the resolved file as a regular file, reject symlinks where the platform permits reliable detection, and enforce a conservative size bound before parsing.
7. The current Rustls API receives private-key bytes in process memory. This design improves at-rest protection and delivery isolation; it does not claim TPM-backed non-exportable signing. A PKCS#11/TPM signer integration is a separate future architecture decision.

## Runtime integration

The runtime JSON accepts exactly one private-key source:
- key: a filesystem path retained for explicit LAB/development compatibility.
- key_credential: a strict systemd credential identifier resolved only beneath CREDENTIALS_DIRECTORY.

The two fields are mutually exclusive. The resolver rejects missing credential environment, invalid identifiers, absent files, symlinks, non-regular files, empty/oversized files, and canonical paths outside the credential directory. config validate and apply both resolve the credential source before TLS key parsing; there is no fallback to key when key_credential is selected. Plans and support bundles expose the source class and credential identifier, never the filesystem path or key bytes.

Encrypted delivery is configured per service instance through a systemd drop-in, not in the shared unit. This preserves compatibility with LAB/file-mode instances and prevents a credential dependency from being imposed on every instance.

## Compatibility and deployment gate

- Ubuntu 22.04 / systemd 249: encrypted systemd credentials are unavailable; production key-custody gate remains open. Do not enable the encrypted-key mode.
- Ubuntu 24.04 / systemd 255: supports systemd encrypted credentials, subject to runtime capability checks and host-specific TPM/host-key provisioning.
- A unit must not declare an unconditional credential dependency that breaks existing file-mode LAB instances. Encrypted credentials are enabled only by an explicit per-instance drop-in or equivalent generated unit configuration.
- Never copy the offline CA private key to a tunnel host.

## Security and acceptance tests

- Missing CREDENTIALS_DIRECTORY, missing credential ID, invalid ID, absent file, symlink, non-regular file, oversized key, and malformed PEM all fail closed.
- Credential identifiers cannot escape the credential directory or introduce path separators.
- Support bundles and plans expose only the selected key-source class and credential ID, never contents or passphrases.
- Unit-level tests cover resolution and rejection cases; Linux integration tests verify systemd credential delivery, permissions, restart behavior, and encrypted credential decryption.
- Hosted Ubuntu 24.04/systemd 255 CI now demonstrates encrypted credential delivery through a real transient systemd unit and successful BAFT runtime TLS-key parsing. This is not persistent service lifecycle evidence. A dedicated operator-controlled Linux host must still demonstrate encrypted credential mode, permissions, restart behavior, upgrade/rollback and uninstall before the production key-custody gate can close.
- No plaintext fallback is permitted when encrypted mode is selected.

## Consequences

The transient encrypted-credential delivery path is now exercised in hosted CI on Ubuntu 24.04/systemd 255, including the actual runtime key parser. This is a staged integration result, not a completed production custody feature. The existing two Ubuntu 22.04 LAB hosts remain suitable for transport and compatibility testing, but cannot exercise the encrypted systemd credential path. Persistent service lifecycle and dedicated-host proof remain open. A production rollout requiring this custody control needs a compatible host/systemd baseline or a separately reviewed hardware-backed signing provider.
