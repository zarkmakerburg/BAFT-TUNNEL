# Release and signing

## Trigger (one line)

```
gh workflow run release.yml            # pre-release (default)
gh workflow run release.yml -f stable=true   # stable
```

Or push a tag `v*` (a tag containing `-`, e.g. `v0.1.0-rc.1`, is a pre-release).
Releases stay **pre-release** until the Phase 1 gates close.

The workflow builds the Linux binary (Rust 1.88.0, `--locked`), writes
`SHA256SUMS` and a CycloneDX SBOM, signs `SHA256SUMS` keylessly with cosign
(GitHub OIDC; **no private key exists anywhere**), self-verifies, then publishes.

## Verify (users)

```
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '^https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum -c --ignore-missing SHA256SUMS
```

## Notes

- Keyless signing records the repo/workflow identity in the public Sigstore
  transparency log (Rekor). If that metadata must stay private, switch to
  minisign with the key stored only as an Environment secret.
- The workflow must be on the default branch for `gh workflow run` to work, and
  `Cargo.toml` must exist on the built ref (Rust code lands via PR #3).
