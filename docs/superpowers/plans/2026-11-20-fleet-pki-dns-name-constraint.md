# Fleet PKI DNS Name Constraint Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Fleet Hub and Member mTLS certificates pass the existing rustls-webpki verifier while retaining a critical CA-enforced Fleet identity namespace and exact URI application identities.

**Architecture:** The Fleet CA constrains DNS SANs to `.fleet.baft.invalid`. Each node and Hub certificate carries one constrained DNS identity SAN paired with its existing URI identity SAN. The Hub's internal DNS identity becomes TLS `hub_name`/SNI, while the real TCP endpoint remains in `host`/`hub_addr`.

**Tech Stack:** Rust, rustls 0.23.46, rustls-webpki 0.103.16, x509-parser 0.16, OpenSSL-backed existing Fleet CA helpers, Tokio.

**Spec:** `docs/superpowers/specs/2026-11-20-fleet-pki-dns-name-constraint-design.md`

## Global Constraints

- Keep `nameConstraints` critical and enforce `DNS:.fleet.baft.invalid`; do not remove or downgrade it.
- Preserve exact `baft-fleet://<id>.fleet.baft.invalid` and `baft-fleet-hub://<id>.fleet.baft.invalid` URI identities.
- Require DNS SAN and URI SAN to encode the same Fleet ID; reject missing, duplicate, extra or mismatched identity SANs.
- Keep the external TCP dial address separate from TLS identity/SNI.
- Retain standard WebPKI chain, validity, server-name, EKU and mandatory CRL verification.
- No custom verifier, new dependency, CAP_NET_ADMIN, firewall, route, sysctl, real-host, merge, release or deployment operation.

## Review Focus

- A Fleet leaf with a DNS SAN outside `.fleet.baft.invalid` must fail chain validation.
- A leaf with a valid URI but missing, duplicate or mismatched DNS SAN must fail application identity validation.
- A Hub offer with a TLS name not derived from `hub_id` must be rejected before connecting.
- The Join TCP host must remain the real advertised address while SNI uses the internal Fleet DNS identity.
- Expired or revoked CRLs must continue to fail closed after the SAN change.

---

### Task 1: Make the Fleet CA and Member leaf DNS-constrained

**Files:**
- Modify: `src/fleet_ca.rs`
- Test: `src/fleet_ca.rs`

**Interfaces:**
- Add `pub fn dns_name(id: &str) -> String`, returning exactly `<id>.fleet.baft.invalid` for a validated Fleet ID.
- Keep `node_uri` and `hub_uri` unchanged.
- Add the CA `subjectKeyIdentifier=hash` required by its CRL `authorityKeyIdentifier=keyid:always` configuration.
- Change CA Name Constraints to critical `permitted;DNS:.fleet.baft.invalid`.
- Add exactly one paired DNS SAN to every `fleet-node` leaf while retaining its URI SAN.

- [x] Write `fleet_ca_initialization_generates_crl_with_issuer_key_identifier`, `fleet_name_constraints_use_critical_permitted_dns_subtree` and `fleet_member_certificate_has_paired_dns_and_uri_sans` tests.
- [x] Run the focused tests and confirm they fail against the current URI-only profile.
- [x] Implement the CA SKI, `dns_name`, the DNS Name Constraints extension, and the paired Member DNS SAN.
- [x] Run focused Fleet CA tests and verify OpenSSL path validation still rejects an out-of-namespace DNS SAN.
- [x] Commit as `fix(fleet): use WebPKI-supported DNS name constraints`.

### Task 2: Separate Hub TLS identity from the external dial address

**Files:**
- Modify: `src/fleet_ca.rs`
- Modify: `src/fleet_helper.rs`
- Modify: `src/fleet_run.rs`
- Modify: `src/fleet_join.rs`
- Test: `src/fleet_ca.rs`, `src/fleet_helper.rs`, `src/fleet_run.rs`, `src/fleet_join.rs`

**Interfaces:**
- Change `fleet_ca::sign_hub(dir, hub_id, csr_pem)` to issue exactly one `DNS:dns_name(hub_id)` and the existing Hub URI SAN; it must not include the external dial DNS/IP.
- `fleet_helper::provision_hub` continues deriving `advertise_host` from `dial_san`, but writes `join.hub_name = fleet_ca::dns_name(hub_id)`.
- `JoinOffer.host` remains the TCP destination; `JoinOffer.hub_name` is the derived internal DNS identity.
- `JoinOffer::validate_fields`, direct `client_connector` calls, Hub Join config and persisted Member config reject any `hub_name` not exactly derived from `hub_id`.

- [x] Write `fleet_hub_certificate_uses_internal_dns_identity_not_dial_address`, `fleet_hub_provision_separates_advertise_host_from_tls_name`, and `fleet_join_offer_requires_derived_hub_tls_identity`, `fleet_join_connector_rejects_non_derived_tls_name` and Member-config identity tests.
- [x] Run the focused tests and confirm the current external-name coupling fails them.
- [x] Update Hub certificate issuance, provisioning config, offer construction/validation and fixtures.
- [x] Verify the TCP destination remains `host:port` while TLS SNI is `<hub_id>.fleet.baft.invalid`.
- [x] Run focused Fleet Join and helper tests.
- [x] Commit as `fix(fleet): separate Hub dial address from TLS identity`.

### Task 3: Enforce paired DNS/URI identities and prove real WebPKI compatibility

**Files:**
- Modify: `src/fleet_hub.rs`
- Modify: `src/fleet_report.rs`
- Modify: `src/fleet_member.rs`
- Modify: `src/fleet_join.rs`
- Modify: `src/tls.rs`
- Test: the corresponding module test suites

**Interfaces:**
- `fleet_hub::peer_identity(leaf_der)` requires exactly one URI for the node ID and exactly one DNS SAN equal to `fleet_ca::dns_name(node_id)`; unexpected or duplicate SAN identities fail.
- `fleet_report::verify_hub_identity(leaf_der, hub_id)` requires exactly one Hub URI and exactly one DNS SAN equal to `fleet_ca::dns_name(hub_id)`.
- Member bundle publication invokes the same strict node identity validation before durable publication.
- Keep `tls::client_config` and `tls::server_config` on the standard WebPKI verifier with mandatory CRL enforcement.

- [x] Write `fleet_identity_rejects_mismatched_dns_and_uri_sans` and `fleet_webpki_accepts_dns_constrained_fleet_hub_and_member_certificates` tests using certificates from the actual Fleet CA helpers.
- [x] Write negative cases for out-of-namespace DNS SAN, missing/duplicate DNS SAN and URI SAN, mismatched URI/DNS IDs, wrong Hub SNI, wrong EKU and revoked/expired CRL.
- [x] Run the new focused tests and confirm the compatibility test fails on the old URI constraint (controlled local baseline: restoring `permitted;URI:.fleet.baft.invalid` made the actual WebPKI mTLS test fail with `AlertReceived(CertificateUnknown)`; all temporary edits were restored).
- [x] Implement strict SAN validation and keep the standard WebPKI verifier path unchanged.
- [x] Run focused tests plus the Fleet Join, Member, Hub and TLS module suites (`cargo test --bins fleet_ -- --nocapture`: 126 passed, 0 failed; macOS compatibility shims restored byte-for-byte).
- [x] Commit as `test(fleet): prove DNS-constrained mTLS verification` (`99a06e0ef9f138a74accb027e6a312a2b886f698`).

### Task 4: Reconcile architecture docs and exact-head evidence

**Files:**
- Modify: `docs/ADR-0051-add-server-from-panel.md`
- Modify: `docs/superpowers/specs/2026-11-20-fleet-pki-dns-name-constraint-design.md`
- Modify: `docs/superpowers/plans/2026-11-20-fleet-pki-dns-name-constraint.md`
- Update: Issue #2 only after code and CI evidence are observed

- [x] Append a dated ADR-0051 addendum superseding the URI Name Constraints detail while preserving the historical decision trail.
- [x] Run `cargo fmt --all -- --check` and `git diff --check`.
- [x] Run the focused Fleet tests locally where supported; record the existing macOS `libc::prctl` limitation. Temporary test-only compatibility shims were restored byte-for-byte; Docker state was untouched.
- [ ] Push the branch and verify all required Linux CI checks on the exact resulting SHA (not performed; requires explicit push authorization).
- [ ] Record exact SHA and exact-head CI run IDs/results after push; current PR #406 checks are for the older remote SHA `8f11dd2c2ed35b4f2fa37ad21d9cfcb8837a0bbe`, not local `99a06e0ef9f138a74accb027e6a312a2b886f698`. No two-host LAB, merge, release, deployment or real-host operation occurred.
