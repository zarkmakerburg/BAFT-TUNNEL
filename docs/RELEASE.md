# Release and signing

## Trigger (one line)

Create and push a version tag on a reviewed commit already contained in `main`:

```
git tag vX.Y.Z <reviewed-main-sha>
git push origin vX.Y.Z
```

The tag push starts the release workflow. To rerun it manually, select that existing
version tag as the workflow ref (or run `gh workflow run release.yml --ref vX.Y.Z`).
A tag containing `-`, e.g. `v0.1.0-rc.1`, is a pre-release. A tag without `-` is a Stable
release: it is cut only on a commit that already has a signed release candidate with
two-host LAB evidence, and only with the owner's explicit approval of the tag push. Stable
releases (v0.2.0 onward) ship with their known issues listed below; evidence levels (CI, VM,
LAB, Production) are never merged into one claim.

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

Set `TAG` to the version being installed, then run:

```
TAG=vX.Y.Z
cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity "https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@refs/tags/$TAG" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum -c --ignore-missing SHA256SUMS
```

The identity is pinned to the release workflow on a semantic-version tag only. Signatures
created by workflow runs on `main` or any branch do not verify. The release workflow itself
requires the selected tag to point to a commit already contained in `main` (SA-03).
Legacy releases signed from `refs/heads/main` are intentionally rejected by the hardened
installer and verifier; publish a new tag after this change is merged.

## Next release: v0.4.0 (Adaptive Traffic Policy Pack) — not yet gated

`v0.3.1` is the latest Stable. Between its tag and `main` only documentation and the
`policy-pack` work merged (#448-#452 and the scheduler-wiring PR), so no `v0.3.2` is planned: a
Stable tag has to carry a product change. The next Stable is `v0.4.0` (roadmap §9.5). It is **not
taggable yet**:

- the policy pack lives behind the `policy-pack` cargo feature, is off by default, and only its
  in-flight window bound (`BAFT_TUNNEL_POLICY_PACK=window-cap[:<bytes>]`, 256 KiB) and its
  scheduler (`BAFT_TUNNEL_POLICY_PACK=sched[:<quantum>[:<cap>]]`) reach the relay; unset means
  today's 4 MiB window, today's pump and today's data path. The scheduler was measured as neither
  a win nor a regression in the two-flow harness scenario, per-MB CPU is unmeasured, and no
  profile has measured evidence, so there is nothing to ship;
- no profile has measured evidence against Balanced and Baseline; the two-host LAB stays
  suspended, so any number produced before an owner-authorized window is harness or CI level
  and must keep the `simulated` label; it is never LAB evidence;
- the gate is the acceptance table in
  [ADAPTIVE_TRAFFIC_POLICY_PACK.md](ADAPTIVE_TRAFFIC_POLICY_PACK.md). A profile that cannot
  beat Balanced is dropped or documented as limited, not shipped as a claim.

When those gates are met the release follows the same train as `v0.3.1`: a signed release
candidate on a reviewed `main`, independent verification of the published assets, then Stable.
A hosted-CI-only evidence level is acceptable only as a recorded residual risk, exactly as for
`v0.3.1`, and never as a substitute for LAB evidence.

## Known issues (Stable)

### v0.3.1 (Stable, released 26 November 2026)

- **Evidence level for this tag: hosted CI only.** It was cut from reviewed `main` whose required
  checks (`rust`, `ubuntu-2204-baseline`, `lab-config`) were green on the tagged commit. The
  two-host Iran/Turkey LAB remains **suspended by owner order**, so no new two-host LAB acceptance
  is claimed for `v0.3.1`. The N-1 upgrade drill (`v0.3.0` to the current build) runs inside the
  hosted `ubuntu-2204-baseline` job (PR #440) and is not a substitute for two-host evidence.
- **S5 is still OPEN.** S5-AUD-01 (whitespace between a header field name and the colon) is fixed
  and merged in PR #446, together with a `read_request` fuzz target and its seed corpus. The
  whole-`main` S5 audit still has **no independent human sign-off**; the owner directed this
  release to proceed without the external review for now. That is a recorded residual risk, not a
  review result, and no document claims S5 is closed.
- Aged Direct flows without `session_rotate_secs` are unchanged from the v0.3.0 known item; rotation
  stays opt-in and 10 s is the tested setting.
- Reverse, the BAFT engine and stream resume remain **Preview**; `ready` and ordinary diagnostics
  `integrity` remain Unknown by design.
- Fleet CA / Join / Pairing (PR #445) is **not** part of this release. It stays open with its own
  acceptance gates and no two-host LAB.
- **Independent verification record (2026-11-26, outside CI).** The published assets of `v0.3.1`
  were downloaded from the release page and checked locally, not trusted from the workflow log:
  `sha256sum -c SHA256SUMS` reported `OK` for all six listed artifacts, and
  `cosign verify-blob SHA256SUMS --bundle SHA256SUMS.cosign.bundle --certificate-identity
  https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@refs/tags/v0.3.1
  --certificate-oidc-issuer https://token.actions.githubusercontent.com` printed `Verified OK`
  with exit status 0. `SHA256SUMS` and `SHA256SUMS.cosign.bundle` are the two remaining assets.
  This confirms provenance of the bytes published for this tag; it does not replace the missing
  two-host LAB acceptance or the S5 review.
- The GitHub release page for `v0.3.1` now carries the same evidence level and known-issue list in
  its body, so the release page alone is not more optimistic than this document.

### v0.2.2

- **Long 2 MiB flows on the Iran LAB path (LAB evidence, cause not fully proven).** In the
  rc.2 to rc.3 upgrade soaks (workflow runs 43897433228 and 43910396414, 16 November 2026),
  probes were intact until about 15:37 UTC and then repeatedly returned truncated payloads
  (for example 704377 of 2097289 bytes) from about 16:00 UTC. The working explanation is that the
  Iran network path throttles flows older than roughly 15 to 20 seconds; paired raw TCP/TLS
  controls outside the tunnel are tracked in the roadmap. It is not yet proven that the product
  is unaffected. Optional Direct session rotation (ADR-0049, off by default) is the planned
  mitigation; LAB results are listed under v0.3.0-rc.4 below.
- The `CertificateRevoked` lines in those logs follow the failure and belong to the end-of-run
  revocation checks; they were not shown to be the cause of the truncated payloads.
- Reverse `tcp-tls` and the BAFT engine remain Preview; `ready` and ordinary `integrity`
  remain Unknown by design.

### v0.3.0 (Stable, released 18 November 2026)

- The same aged-flow limit as rc.4 applies to default configurations (session rotation off). Enable `session_rotate_secs` for Iran-to-abroad Direct sessions; see rc.4 below.
- Reverse, BAFT engine and stream resume remain Preview.

### v0.3.0-rc.4 (prerelease)

- **Aged Direct flows without rotation (LAB evidence).** The v0.2.2 to rc.4 upgrade LAB without
  rotation (run 44257587909, 18 November 2026) passed signature checks, upgrade and rollback
  preflight, the cross-version restart and the 0.3.0 feature checks, then returned 589716 of
  2097289 bytes on the first post-upgrade 2 MiB probe, the same signature as v0.2.2; the soak did
  not run. Direct session rotation (ADR-0049, `session_rotate_secs`, off by default) avoids it in
  LAB: the same upgrade with rotation at 10 s (run 44271619864) completed the one-hour soak with
  278 rounds, 0 failed probes, 0 restarts and flat fd/RSS, and the tcp-tls and Noise-TCP rotation
  probes passed (runs 44269675131 and 44266349971, off 0 of 3, on 3 of 3). Rotation is opt-in,
  so default configurations still show the limit until it is enabled.
- Evidence is LAB on the Iran path for Direct only. Reverse, BAFT and stream resume (protocol v3,
  default off, Preview; chaos LAB run 44283746412) are not covered by these results.

### v0.3.1-rc.1 (prerelease, not Stable)

- Tag `v0.3.1-rc.1` (`4c2a5bc`). Prerelease channel only; the README one-line install still pins `v0.3.0`.
- Doctor: LAB PASS, one run ([44351670022](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44351670022)).
- Upgrade LAB `v0.3.0` to `v0.3.1-rc.1` with session rotation 10 s (1 h soak): LAB PASS, one run ([44368821156](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44368821156)).
- Upgrade LAB with rotation 30 s: FAIL ([44353076174](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44353076174)). 3 of 243 two-megabyte probes were cut mid-transfer (34-38 s, connection reset on the draining session); 0 restarts, flat fd and memory. Not accepted as a path outage. Rotation is opt-in and 10 s is the tested setting.
- Pairing LAB for this tag is not recorded. Do not claim it until it is.
- Fleet is not enabled for remote management; enrollment confirm/sign is not yet tested end-to-end (see roadmap §9.9).

## Notes

- **v0.3.1 release train (2026-11-26).** `main` carried five merged PRs (#442, #443, #444, #441,
  #446) plus the docs record in #447. The signed prerelease `v0.3.1-rc.2` and the Stable tag
  `v0.3.1` point at the same commit `3b8bcde7cdcde64000aa199173437f511d3731b5`, so the Stable
  binary is the rc binary; both releases carry the eight signed assets. The checklist file
  [STABLE_TAG_CHECKLIST.md](STABLE_TAG_CHECKLIST.md) records the v0.2.2/v0.3.0 procedure that this
  train follows; the deviations are the evidence level (hosted CI, no two-host LAB) and the
  owner-directed waiver of the independent review for S5, both recorded above.
- Keyless signing records the repo/workflow identity in the public Sigstore
  transparency log (Rekor). If that metadata must stay private, switch to
  minisign with the key stored only as an Environment secret.
- The release workflow must exist on the default branch. Manual runs must select an existing
  version tag; the tag commit must be contained in `main`.

## Installer trust root (offline verification)

`quick-install.sh` verifies the release signature online first. When the Sigstore TUF CDN is unreachable, for example from Iran, it verifies the same bundle with the same pins against the Sigstore trust root pinned in the script (ADR-0034). Refresh that pin with `scripts/sigstore/refresh-trust.sh --update` whenever Sigstore rotates keys, and before the Fulcio chain expires; `--check` prints the date. Review the diff and ship it in the next release.
