# B4 BAFT Guided Setup Implementation Plan

> For agentic workers: REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Enable a verified, Direct-only BAFT engine to be installed and configured through the existing BAFT-TUNNEL installer and Wizard without claiming unsupported connection health or traffic evidence.

**Architecture:** Keep BAFT as the separately built, pinned Go engine process supervised by the Rust baft-tunnel@ service. Install the engine into the existing Engine Store from the signed release package; derive its exact digest from the verified current store entry and bind that digest into setup state, pairing bundles, and final configuration. Extend the existing offline-PKI flow with a distinct BAFT certificate identity profile, then expose BAFT setup only when the installed engine verifies.

**Tech Stack:** Rust 1.88, Tokio, serde/serde_json, existing engine_store and baft_engine modules, OpenSSL shell PKI helper, Bash installer, vanilla JS bilingual panel, GitHub Actions.

**Spec:** docs/BAFT_ENGINE_INTEGRATION_DESIGN.md (identity, Engine Store, Phase F); docs/BAFT_ENGINE_ACCEPTANCE.md; docs/ADR-0019-baft-engine-host.md; B4 in docs/REMAINING_WORK_PLAN.md.

## Global Constraints

- BAFT remains a separate pinned AGPL-3.0 executable; do not import BAFT Go source into the Rust core or modify the upstream BAFT repository.
- BAFT is Preview and Direct-only; Reverse and protocol v2 remain unavailable.
- BAFT node certificates use exactly one URI SAN urn:baft:node:<node_id> plus exactly one configured DNS/IP server SAN; tcp-tls keeps urn:baft-tunnel:node:<node_id>.
- BAFT setup requires the existing offline CA, current CRL, node private key mode 0600, mutual TLS, CA-chain validation, and exact BAFT peer URI identity (`urn:baft:node:<peer_id>`). BAFT does not support an enforced peer-certificate fingerprint pin (design risk R12); do not add one to the config.
- The engine binary must match the exact SHA-256 in release/engine-manifest.json; the installed Engine Store is root-owned and not writable by the service account.
- A repeated product install must preserve an already verified current engine release; it must not silently switch an active tunnel to a different engine digest. An invalid existing store fails closed.
- The installer must not enable or start a tunnel. Starting remains an explicit Wizard/Apply action.
- BAFT process readiness is only a TCP-listener observation. End-to-end status, authorized session, payload traffic, and integrity remain unknown until BAFT publishes independently verifiable evidence.
- Keep the generic baft-tunnel@.service; do not add a second BAFT systemd service or duplicate lifecycle path.
- B3 firewall inspection remains blocked. Do not add CAP_NET_ADMIN, firewall mutation, or a privileged firewall helper as part of B4.
- No real-host B4 LAB is authorized by this plan. Request separate owner authorization before dispatching any two-host or real-host B4 LAB.
- Preserve existing worktrees and user changes. No reset, clean, force-push, stable release, production deploy, or Phase 2 transition.

## Review Focus

- A tcp-tls certificate submitted as BAFT (or vice versa) must be rejected before the active certificate pointer changes.
- A missing, corrupt, or changed Engine Store digest must disable BAFT setup or fail pairing without writing a final config.
- A peer bundle with a different BAFT engine digest must be rejected even when the engine name matches.
- BAFT Reverse and protocol v2 must remain rejected by CLI, API, Preflight, and Wizard.
- A running BAFT process or open TCP listener must never be reported as an authenticated session, end-to-end-ready tunnel, or observed traffic.
- Reinstalling a newer package must not silently replace the engine digest used by an existing tunnel.

---

### Task 1: Add an explicit BAFT certificate identity profile

**Files:**
- Modify: src/pki_helper.rs
- Modify: src/tls.rs
- Modify: src/agent.rs (PKI request/receipt validation)
- Modify: scripts/pki/baft-pki.sh
- Modify: panel/pages.js (CSR/install request and offline CA command)
- Test: existing Rust PKI helper tests and PKI shell tests

**Interfaces:**
- Add CertificateProfile::{TcpTls, Baft} with snake_case serialization and TcpTls as the backward-compatible default.
- Add profile to PKI helper requests and CSR metadata. New metadata writes version 2; metadata version 1 without profile reads as TcpTls.
- Extend request_document and certificate_request_document to carry the profile.
- Add tls::node_certificate_summary_for_profile(cert, node_id, profile), or equivalent, to check the exact URI prefix and return certificate pin/server name.

- [ ] Add tests proving TcpTls accepts only urn:baft-tunnel:node:<id> and Baft accepts only urn:baft:node:<id>, with exactly one URI SAN and one expected DNS/IP SAN.
- [ ] Run focused PKI tests and confirm cross-profile and extra-SAN cases fail before implementation.
- [ ] Implement the profile in PkiRequest, CsrMetadata, CSR create/read, and certificate installation. Keep old requests and metadata defaulting to TcpTls.
- [ ] Extend Agent CSR and certificate request bodies with the enum profile; when a config or setup state exists, require the requested profile to match its engine. Before Init, accept only the two explicit enum values.
- [ ] Extend baft-pki.sh issue-node and verify-node with an explicit --profile baft option while preserving the existing default tcp-tls command. Allow revoke-node to revoke certificates from either supported URI profile.
- [ ] Update the Wizard to send the selected profile and show the matching copyable offline CA command.
- [ ] Run focused Rust PKI tests and PKI shell tests; verify old tcp-tls enrollment behavior remains unchanged.
- [ ] Commit: feat(pki): support BAFT node identity profile.

### Task 2: Make setup plan/init/join support pinned BAFT Direct

**Files:**
- Modify: src/engine_store.rs
- Modify: src/baft_engine.rs
- Modify: src/setup.rs
- Modify: src/agent.rs (setup command generation)
- Modify: src/config.rs only if a schema adjustment is required
- Test: engine_store, setup, and setup-helper unit tests

**Interfaces:**
- Add engine_store::DEFAULT_STORE_PATH with value /usr/local/lib/baft-tunnel/engine-store.
- Add engine_store::current_sha256(store: &Path, id: &str) -> Result<String>; it verifies current symlink, manifest, artifact hash, file type, and mode.
- Extend pairing Bundle with optional engine_sha256; use bundle version 2 for BAFT bundles and keep version 1 byte-compatible for tcp-tls/Noise bundles.
- Extend persisted setup State with optional engine_sha256, defaulting to None for existing state files.

- [ ] Add Engine Store tests for verified current digest, missing current, broken symlink, manifest mismatch, tampered binary, and writable binary.
- [ ] Add setup tests for BAFT Direct plan in both roles, missing Engine Store, Reverse rejection, protocol-v2 rejection, and generated baft.store/baft.sha256 values.
- [ ] Implement a BAFT setup-settings resolver using the verified current store digest; do not accept caller-supplied binary path or digest.
- [ ] Change setup TLS-material selection so tcp-tls and baft require PKI material, while only noise-tcp generates a Noise key.
- [ ] Make BAFT init validate the BAFT certificate profile, persist the BAFT node identity and serving-node DNS SAN, and persist the exact engine digest in state and bundle. Do not treat the certificate fingerprint as an authorization pin; BAFT authorizes by CA trust and exact URI identity.
- [ ] Make BAFT join require bundle version 2, matching engine_sha256, matching engine/direction/protocol/tunnel/roles, and peer certificate fingerprint. Resolve peer server name from the serving bundle; never substitute a Noise public key for BAFT.
- [ ] Keep adopt_for_service and generated root commands on node.key for both TLS engines; retain node.noise only for Noise.
- [ ] Add tests proving a changed local store digest or mismatched peer digest refuses join and writes no final config; verify successful two-node BAFT pairing produces valid runtime configs.
- [ ] Run focused setup and Engine Store tests; run the full Rust suite on supported Linux CI after integration.
- [ ] Commit: feat(setup): support pinned BAFT direct pairing.

### Task 3: Package and install the signed BAFT Engine Store

**Files:**
- Modify: .github/workflows/release.yml
- Modify: scripts/install.sh
- Modify: scripts/test-install.sh and/or release-package smoke checks
- Review: scripts/rollback.sh and scripts/uninstall.sh (Engine Store stays inside the existing managed library directory)

**Interfaces:**
- Release package paths: engines/baft-linux-amd64, engines/licenses/, and release/engine-manifest.json.
- Installed store path: /usr/local/lib/baft-tunnel/engine-store.
- Installer invokes the packaged baft-tunnel engine install/verify commands; no network download is added to the host installer.

- [ ] Reorder release staging so the pinned BAFT build completes before the main package archive is assembled.
- [ ] Include the BAFT binary, full license/source bundle, and engine manifest inside the main install archive. The archive remains covered by signed SHA256SUMS verification in quick-install.sh.
- [ ] Add installer handling for all-or-none engine package assets. On fresh install, install and verify BAFT into Engine Store. On reinstall/upgrade, preserve and verify an existing current store instead of changing its digest; fail closed if an existing current store is invalid. If source-install assets are entirely absent, leave BAFT unavailable rather than inventing an engine.
- [ ] Ensure installer copies the engine manifest and license bundle into the managed library directory and keeps the store root-owned/read-only to the service account.
- [ ] Add release-package smoke assertions for the installed current entry, successful engine verify, license files, and no tunnel auto-start.
- [ ] Add a reinstall smoke proving an existing verified store digest is preserved.
- [ ] Verify rollback restores the previous library directory including Engine Store, and uninstall removes it through the existing managed-directory lifecycle.
- [ ] Run shell syntax checks, installer tests, and release package smoke on Ubuntu 22.04.
- [ ] Commit: feat(installer): ship and verify the pinned BAFT engine.

### Task 4: Expose BAFT setup only when installed and keep telemetry truthful

**Files:**
- Modify: src/capabilities.rs
- Modify: src/agent.rs
- Test: capability, status, diagnostics, and traffic tests in src/agent.rs and src/capabilities.rs

**Interfaces:**
- Add a capability-catalogue builder that accepts verified BAFT setup availability and a non-sensitive unavailable reason.
- GET /v1/engines reports guided_setup.available=true only when engine_store::current_sha256(DEFAULT_STORE_PATH, "baft") succeeds.

- [ ] Add tests for missing/corrupt store => BAFT setup unavailable, and verified store => BAFT setup available.
- [ ] Update BAFT identity capability facts to CA=true, CRL=true, SNI=required, URI SAN verified, SHA-256 peer pin, and offline-CA/CRL validation; keep Preview and Direct-only.
- [ ] Add BAFT-specific status tests with an active unit, open listener, and deliberately planted stale session/counter evidence. Tunnel state, authorized session, data path, integrity, and traffic must remain unknown; unit state and raw TCP reachability may be reported separately.
- [ ] Make BAFT diagnostics treat absence of BAFT session/counter telemetry as unknown, never as an observed session or failed authorization. Do not infer authentication from the supervisor TCP readiness probe.
- [ ] Run focused API/capability tests.
- [ ] Commit: fix(status): keep BAFT readiness and traffic unknown.

### Task 5: Enable the BAFT Wizard path without adding user complexity

**Files:**
- Modify: panel/pages.js
- Modify: panel/i18n.js
- Modify: scripts/panel-qa/wizard-engines.js
- Modify: scripts/panel-qa/browser-flows.js only where needed

**Interfaces:**
- The Wizard consumes dynamic /v1/engines capability response; no client-side guess that BAFT is installed.
- BAFT local certificate server SAN is derived as <node_id>.baft-tunnel.invalid; forward peer SNI is derived from the peer bundle, not an extra user field.

- [ ] Add bilingual QA cases: BAFT disabled with missing store; enabled with verified store; Preview label; Direct only; Reverse absent; no Ready wording.
- [ ] Make PKI enrollment required for tcp-tls and baft, but not Noise. Derive BAFT local server SAN from node ID and send profile=baft for CSR creation and certificate installation.
- [ ] Display BAFT-specific offline CA issue command; retain existing tcp-tls command unchanged.
- [ ] Keep existing one-service-port guided flow and avoid asking BAFT users for a server-name value that can be derived safely.
- [ ] Exercise plan, init, bundle export/import, join, and explicit Apply confirmation through the existing root setup helper; no installer auto-start.
- [ ] Run bilingual mobile/desktop panel QA and ensure BAFT status/traffic copy remains explicitly unknown.
- [ ] Commit: feat(panel): enable verified BAFT direct setup.

### Task 6: Reconcile acceptance docs and CI evidence

**Files:**
- Modify: docs/BAFT_ENGINE_INTEGRATION_DESIGN.md
- Modify: docs/BAFT_ENGINE_ACCEPTANCE.md
- Modify: docs/PHASE1_IMPLEMENTATION_STATUS.md and docs/REMAINING_WORK_PLAN.md only for evidence-backed state changes
- Modify relevant CI workflow only if a required focused test is not already covered

- [ ] Correct the design PKI section to distinguish tcp-tls and BAFT URI SAN prefixes; reconcile installer package paths and generic baft-tunnel@ supervisor with existing code.
- [ ] Record B4 code/CI evidence separately from existing BAFT engine LAB evidence. Do not claim Wizard-path two-host acceptance from the earlier engine-only LAB.
- [ ] Run full Rust suite on supported Ubuntu 22.04 CI, PKI shell tests, installer/package smoke, panel QA, BAFT conformance/golden checks, and same-SHA release package verification.
- [ ] Keep the B4 PR Draft until a separate owner-authorized two-host B4 LAB proves setup through Wizard, pairing, authenticated data path, and clean stop/uninstall. Do not dispatch that LAB under the B2-only authorization.
- [ ] Record exact PR head SHA, CI runs, and remaining acceptance blocker in the HQ Board.
- [ ] Commit: docs(baft): record B4 guided setup evidence and gates.

---

## Execution Notes

- Use the isolated worktree work/b4-baft-engine-setup; do not reuse or alter existing B2 worktrees.
- The Mac baseline cargo test --locked ran on clean origin/main and produced 209 passes / 5 failures. The failures are platform/environment-specific candidates (Engine Store linux-amd64 guard, macOS ownership expectations, OpenSSL key-format assertion) plus one diagnostics fixture mismatch; treat them as pre-existing baseline failures, not B4 regressions. Linux CI is the acceptance environment.
- Do not run a real-host LAB until the owner explicitly authorizes the B4 workflow. A CI simulation is not a substitute for the two-host LAB.


## Locked Interfaces and Verification Commands

- Define CertificateProfile in src/tls.rs, with TcpTls as serde/default compatibility value and Baft as the second value. The profile-aware certificate summary must parse the leaf certificate directly and enforce the selected exact URI SAN; do not call the tcp-tls-only identity verifier for BAFT.
- Offline CA syntax is: baft-pki.sh issue-node --profile baft CA_DIR NODE_ID CSR_FILE OUTPUT_DIR SERVER_NAME [VALID_DAYS] and baft-pki.sh verify-node --profile baft CA_DIR CRL_FILE NODE_CERT NODE_ID SERVER_NAME. Omitting --profile preserves tcp-tls behavior. revoke-node accepts either supported URI prefix.
- Pairing version 1 remains unchanged for tcp-tls and Noise. BAFT init emits version 2 and a mandatory engine_sha256; BAFT join rejects version 1, missing digest, non-BAFT version 2, or any digest mismatch.
- Add an Apply lifecycle test proving BAFT uses the existing baft-tunnel@NAME unit for explicit start and stop, and that no successful Apply receipt claims an authenticated session.
- Focused Rust commands after each task:
  - cargo test --locked pki_helper::tests::certificate_profile_is_bound_to_csr_and_certificate
  - cargo test --locked engine_store::tests::current_sha256_verifies_current_release
  - cargo test --locked setup::tests::baft_direct_pairing_uses_same_pinned_engine_digest
  - cargo test --locked agent::tests::baft_status_and_traffic_stay_unknown
  - cargo test --locked apply::tests::baft_uses_generic_instance_unit_for_start_and_stop
- PKI shell verification: bash scripts/pki/test-baft-identity-profile.sh (new focused test covering both profiles, mismatch rejection, and revocation). Installer verification: bash scripts/test-install.sh plus the release.yml package smoke on Ubuntu 22.04.
- Panel verification: run the existing bilingual Wizard QA with its test Agent and token file; assert both missing-store and verified-store catalogue states, and do not click Apply on a real host during browser QA.
- Final acceptance is the exact-SHA Linux CI suite plus BAFT conformance/package verification. A real two-host B4 LAB remains a separate owner-authorization gate.


## Review Corrections (supersede conflicting wording above)

- **BAFT identity is not the tcp-tls identity.** BAFT requires exactly `urn:baft:node:<node_id>`; tcp-tls remains `urn:baft-tunnel:node:<node_id>`. This is grounded in `docs/BAFT_ENGINE_INTEGRATION_DESIGN.md` R15 and BAFT's `internal/node/node.go`. The same CA can issue both profiles, but their URI SANs are distinct.
- **Do not pin the BAFT peer certificate fingerprint.** BAFT's `validate_baft` explicitly rejects `peer_fingerprint` on a listener and design risk R12 states BAFT has no peer certificate pin; its trust model is CA chain + exact peer URI identity. A bundle field may retain an audit fingerprint only if its semantics are clearly non-authoritative; it must never be represented as a BAFT enforcement pin.
- **Engine Store path is a product constant to define, not an existing CLI default.** `engine install/verify` currently requires an explicit `--store`, and `BaftSettings.store` is optional. Task 2 must define one canonical absolute path and use it consistently in installer, setup-generated `baft.store`, agent availability checks, and tests. The proposed `/usr/local/lib/baft-tunnel/engine-store` is acceptable only after checking it against the existing library swap lifecycle.
- **Installer preservation is a first-class requirement.** `scripts/install.sh` currently stages and replaces the entire `/usr/local/lib/baft-tunnel` directory. If Engine Store lives under `LIB_DEST`, the installer must copy and verify the existing store into the staged replacement before swapping; otherwise every normal reinstall would erase it. Rollback must restore the prior library including the store.
- **No certificate fingerprint is required in the BAFT pairing protocol.** `Bundle.public` currently has Noise/tcp-tls semantics. Task 2 must explicitly define a backward-compatible BAFT meaning or omit/replace that field only for BAFT bundle version 2; it must not accidentally map it to `peer_fingerprint`.
- **`/v1/engines` already exposes BAFT as Preview/Direct-only; the missing capability is setup availability.** Task 4 should preserve current catalogue metadata and only gate `guided_setup.available` on a verified Engine Store. Status/traffic must remain `unknown`, as already required by `docs/BAFT_ENGINE_ACCEPTANCE.md` A11.
