# Release and signing

## Trigger (one line)

Create and push a version tag on a reviewed commit already contained in `main`:

```
git tag vX.Y.Z <reviewed-main-sha>
git push origin vX.Y.Z
```

The tag push starts the release workflow. To rerun it manually, select that existing
version tag as the workflow ref (or run `gh workflow run release.yml --ref vX.Y.Z`).
A tag containing `-`, e.g. `v0.1.0-rc.1`, is a pre-release. Releases stay
**pre-release** until the Phase 1 gates close.

The workflow builds the Linux binary (Rust 1.88.0, `--locked`), writes
`SHA256SUMS` and a CycloneDX SBOM, signs `SHA256SUMS` keylessly with cosign
(GitHub OIDC; **no private key exists anywhere**), self-verifies, then publishes.

Assets per release: the bare binary, the install package
`baft-tunnel-TAG-linux-x86_64.tar.gz` (binary, `scripts/*.sh`, `scripts/pki/*.sh` without tests,
`systemd/`, `QUICKSTART.en.md`/`QUICKSTART.fa.md`), the pinned BAFT engine and licences, the
engine manifest, the SBOM, `SHA256SUMS` and its cosign bundle. Every file is listed in the signed
`SHA256SUMS`. Before publishing, the workflow extracts the package and runs its own
`scripts/install.sh --destdir` to prove it installs without the repository. Install steps:
[quickstart.en.md](quickstart.en.md) / [quickstart.fa.md](quickstart.fa.md).

## Verify (users)

```
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '^https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@refs/tags/v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum -c --ignore-missing SHA256SUMS
```

The identity is pinned to the release workflow on a semantic-version tag only. Signatures
created by workflow runs on `main` or any branch do not verify. The release workflow itself
requires the selected tag to point to a commit already contained in `main` (SA-03).
Legacy releases signed from `refs/heads/main` are intentionally rejected by the hardened
installer and verifier; publish a new tag after this change is merged.

## Notes

- Keyless signing records the repo/workflow identity in the public Sigstore
  transparency log (Rekor). If that metadata must stay private, switch to
  minisign with the key stored only as an Environment secret.
- The release workflow must exist on the default branch. Manual runs must select an existing
  version tag; the tag commit must be contained in `main`.
