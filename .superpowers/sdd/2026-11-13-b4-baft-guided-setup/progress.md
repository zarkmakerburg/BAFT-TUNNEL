# SDD ledger — B4 BAFT guided setup

Base: `b12f4a9f14da32d0c2cd0a351a31e231abb5f52c` on `work/b4-baft-engine-setup`.

## Implemented in the current uncommitted worktree
- Certificate profiles: `tcp_tls` remains the legacy default; `baft` uses `urn:baft:node:<id>`. CSR metadata v2 stores the profile; v1 metadata remains TCP-TLS only. Agent CSR/install requests carry the profile, and stored CSR/certificate verification binds it.
- Offline PKI CLI supports explicit `--profile baft` for issue/verify; revocation accepts both supported identity prefixes.
- BAFT setup plan supports Direct Serve/Forward, rejects Reverse and protocol v2, uses the fixed `/usr/local/lib/baft-tunnel/engine-store`, and resolves only a verified current digest. BAFT peer authorization uses CA trust plus exact URI identity; `peer_fingerprint` is rejected on both BAFT roles and no certificate fingerprint pin is emitted. BAFT uses PKI key/cert/CA/CRL rather than Noise material.
- BAFT init validates the BAFT URI profile and the listener SAN `<node_id>.baft-tunnel.invalid`; state and version-2 pairing bundles bind the exact Engine Store SHA-256. Join and bundle preview reject version/digest mismatch and recheck the local digest before final config write. Forward SNI comes from the serving bundle, not a separate Wizard input.
- `/v1/engines` marks BAFT guided setup available only when the Engine Store current release verifies. BAFT stays Preview/Direct-only; status, session, payload, integrity and traffic evidence explicitly ignore planted Rust-engine telemetry and remain Unknown. Unit state and listener/raw TCP reachability remain separately observable.
- Installer preserves and verifies an existing store, fails closed on invalid store/package assets, installs all packaged assets on fresh package install, and keeps the store under the managed root-owned library path. Release package workflow now includes engine binary, manifest and license bundle and verifies staged installation.
- Wizard sends profile-specific PKI requests, requires PKI for BAFT, derives the BAFT local SAN, and avoids asking for forward SNI.

## Verified on Mac worktree
- `cargo test --locked --all-targets --no-fail-fast` on macOS with five Linux/OpenSSL-specific tests filtered: 217 passed, 0 failed; protocol contract: 6 passed, 0 failed; bench target compiled and enumerated.
- Focused setup/PKI/capability tests passed; `tls::tests::baft_certificate_summary_requires_one_exact_uri_and_derived_dns_san` passed; cross-profile and extra-URI SAN rejection tests passed.
- `cargo clippy --locked --all-targets -- -D warnings`: PASS.
- Engine Store missing-current digest test passed; broken-current-link regression is Linux-only and added for CI.
- `scripts/test-install.sh`: PASS, including mocked preservation of an existing store through upgrade and rollback, and fail-closed invalid-store behavior.
- `scripts/pki/test-baft-identity-profile.sh`: PASS (BAFT/TCP-TLS profile issuance, cross-profile rejection, dual-profile revocation). Added to `phase1-rust` CI.
- Extended `scripts/panel-qa/pki-enrollment.js` for English/Persian BAFT Wizard selection, Preview-only label, Direct-only/Reverse-disabled, derived SAN/no separate SNI fields, no peer fingerprint, and profile-bound CSR. Browser execution pending PR Playwright CI.
- Linux-only BAFT status regression test added to prove planted live-PID Rust session/counter and stale integrity receipts do not promote BAFT tiers; not executable on macOS, scheduled for Ubuntu CI.
- `cargo fmt --check`, `git diff --check`, `bash -n` for installer/PKI/test scripts, and `node --check panel/pages.js`: passed on latest relevant revisions.

## Not yet proven / remaining gates
- Linux Ubuntu 22.04 CI has not run; release-package engine install smoke depends on Linux-amd64 Engine Store verification.
- No real BAFT certificate issuance/installation two-node setup or two-host LAB was run. Do not claim B4 acceptance or Apply readiness.
- Add changed local/peer digest no-write pairing tests and full BAFT CSR→offline issue→install integration tests.
- Execute Linux-only Engine Store and BAFT status regression tests in Ubuntu CI; complete bilingual panel QA, docs/acceptance reconciliation, and same-SHA release package review.
- Integration commit `8c062485c8e5094acdb58806b20956e2843fe1d2` is pushed and Draft PR #197 is open. The latest bilingual Wizard QA extension is a follow-up commit in progress. No merge, release, deploy, host mutation, or Phase transition has occurred.
