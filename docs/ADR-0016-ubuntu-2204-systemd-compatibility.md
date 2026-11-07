# ADR-0016: Ubuntu 22.04 baseline and systemd credential compatibility

- Status: Accepted
- Date: 2026-11-07
- Scope: Phase 1 Linux host support and node-key delivery

## Context

The two configured LAB hosts are Ubuntu 22.04 with systemd 249.11. Ubuntu 22.04 is the project's default supported server baseline. The previous credential-custody decision made systemd 250+ encrypted credentials appear to be a universal requirement, which would unnecessarily exclude the actual baseline and force an OS/systemd upgrade.

## Decision

1. Ubuntu 22.04 LTS with its supported systemd 249 series is the default supported Linux baseline for BAFT-TUNNEL. Do not require Ubuntu 24.04, systemd 250+, or a manual systemd upgrade.
2. On systemd 247+ hosts, the baseline service-credential mechanism is `LoadCredential=`. Its source key file MUST be root-owned, mode 0600, inside a root-owned directory mode 0700. The systemd manager stages a service-scoped credential for the unit; the application reads it only via `CREDENTIALS_DIRECTORY`. The credential source is plaintext at rest: this mode provides access control and service-scoped delivery, not encryption at rest.
3. `LoadCredentialEncrypted=` remains an optional enhanced mode on systemd 250+ only. It must be explicitly selected and capability-checked. It may use TPM2/host-key encryption according to systemd's supported configuration. It is not required for baseline compatibility.
4. If encrypted mode is explicitly selected and unavailable or invalid, fail closed. Never silently downgrade that selection to `LoadCredential=` or a filesystem-key source.
5. The two modes share the existing strict `key_credential` runtime resolver. Credential plaintext must not be placed in JSON, environment variable values, command-line arguments, logs, plans, or support bundles.
6. Keep the existing filesystem `key` mode only as an explicitly identified development/LAB compatibility path. It is not the default custody recommendation.
7. Do not upgrade the current Ubuntu 22.04 LAB hosts to systemd 250 or perform a distribution upgrade to satisfy the optional encrypted mode.

## Security properties and limitations

The baseline credential source remains plaintext on disk and is protected by root-only ownership/mode and filesystem access controls. Root compromise, offline disk access without full-disk encryption, and privileged host compromise remain in scope. systemd's per-service credential staging reduces exposure to unrelated unprivileged processes but does not make the key non-exportable. BAFT-TUNNEL's Rustls stack still loads key material into process memory. This ADR does not claim TPM-backed signing or encrypted-at-rest storage for the baseline mode.

## Acceptance

- Ubuntu 22.04/systemd 249 must be supported without upgrading systemd.
- Baseline credential provisioning and service lifecycle must be exercised on systemd 249 using `LoadCredential=`.
- Tests must verify root-only source directory/file permissions, successful runtime resolution through `CREDENTIALS_DIRECTORY`, fail-closed behavior when the credential is missing, and clean stop/restart.
- systemd 250+ encrypted credential tests remain a separate optional capability test, not a baseline gate.
- Docs, installer guidance, and active acceptance status must distinguish baseline support from optional encrypted-at-rest delivery.
