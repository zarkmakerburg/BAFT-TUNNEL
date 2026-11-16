# ADR-0034: Installer signature verification without the Sigstore TUF CDN

Status: accepted (2026-11-16)

## Context
`scripts/quick-install.sh` verifies the signed `SHA256SUMS` with a pinned Cosign (`verify-blob`, keyless, exact `release.yml@refs/tags/$TAG` identity and GitHub OIDC issuer). Cosign fetches its trust root (Fulcio CA, Rekor and CT log keys) online from `tuf-repo-cdn.sigstore.dev`. On the Iran LAB host that CDN answers HTTP 403 (LAB run 43850430319), so the one-line installer could not install on any Iran server, while GitHub (installer, Cosign package, release assets) stayed reachable.

## Decision
- Keep the online path first. If it fails, verify the **same bundle with the same identity and issuer pins** again with `--offline` against a Sigstore trust root pinned in the installer. Cosign then makes no network request. The trust root covers the Fulcio CA chain, the `rekor.sigstore.dev` key and the current CT log key, all passed through Cosign's `SIGSTORE_*` variables in an isolated `HOME`.
- Both paths fail closed. A tampered file, a foreign identity or a bad bundle fails both, and the installer stops with "nothing was installed". There is no checksum-only or unsigned fallback.
- Source of the pinned root: `scripts/sigstore/trusted_root.json`. It is the TUF-verified copy Cosign writes during `cosign initialize`, and the PEM block in `quick-install.sh` is generated from it by `scripts/sigstore/refresh-trust.sh`. CI runs `refresh-trust.sh --check`, so the block cannot drift from the JSON.
- The installer prints which path verified the signature.

## Refresh and expiry
- Run `scripts/sigstore/refresh-trust.sh --update` with the pinned Cosign when any of these happens:
  - Sigstore announces a Fulcio, Rekor or CT key rotation.
  - The online path starts failing on a reachable host.
  - The Fulcio chain approaches expiry. `--check` prints the date; it is currently 2031.
- Review the diff like any trust change, and ship it in the next release.
- Releases signed after a rotation still verify online. Offline verification of those releases needs the refreshed root, and it fails closed until then.

## Evidence
- Local sandbox:
  - With the TUF CDN unreachable, the `v0.2.1-rc.1` bundle verifies offline, and the installer's `--verify-only` takes the offline path and passes.
  - A wrong tag identity and a modified `SHA256SUMS` are both refused offline.
- LAB: the Iran-host run is recorded with the 0.2.2 evidence.
