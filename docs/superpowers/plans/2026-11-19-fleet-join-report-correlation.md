# Fleet Join First-Report Correlation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enroll a pending Fleet offer only after the Hub accepts a first mTLS report from the exact certificate delivered for that offer.

**Architecture:** Store the delivered leaf's SHA-256 fingerprint in the Enroller. Derive the peer fingerprint from authenticated TLS certificate DER and invoke a HubStore callback only after the report rules accept the body. The Ctx implementation correlates node ID + fingerprint and advances the Enroller.

**Tech Stack:** Rust, Tokio, rustls, x509-parser, SHA-256, existing Fleet Hub/Enroller modules.

**Spec:** `docs/superpowers/specs/2026-11-19-fleet-join-report-correlation-design.md`

## Global Constraints

- No `CAP_NET_ADMIN`, firewall, route, sysctl or real-host changes.
- No merge, release, deploy, or two-host LAB in this slice.
- Keep the Fleet CA key inside the root helper; Agent handles public certificate material only.
- Reverse remains unavailable; no Go in core; no fake/demo data.
- Do not mark Enrolled on certificate issuance or Join persistence ACK alone.

## Review Focus

- A different certificate for the same node ID must not complete the offer.
- A rejected/replayed/too-fast report must not trigger enrollment.
- Missing or malformed TLS peer identity must fail closed before report handling.
- Concurrent report acceptance and offer denial must not deadlock or resurrect a closed offer.
- An old Enrolled offer must not match a later offer for the same node.

---

### Task 1: Bind Enroller delivery to the exact certificate

**Files:**
- Modify: `src/fleet_enroll.rs`
- Test: `src/fleet_enroll.rs` unit tests

**Interfaces:**
- Produces: `Enroller::mark_delivered(id: &str, now: u64) -> Result<(), Error>; derives the fingerprint from the stored issued certificate and rejects expired delivery`
- Produces: `Enroller::acknowledge_first_report(node_id: &str, certificate_fingerprint: &str, now: u64) -> bool`

- [x] Add tests proving a Join delivery ACK stores the fingerprint, wrong node/fingerprint cannot enroll, and an expired offer cannot be delivered or enrolled.
- [x] Run the focused Enroller test and verify it fails for the expected missing signature/behavior.
- [x] Add an optional stored fingerprint to each offer; parse the stored issued certificate as exactly one X.509 PEM leaf and derive its SHA-256 digest internally.
- [x] Change `mark_delivered` to store the fingerprint and enter `AwaitingReport`.
- [x] Change first-report acknowledgment to match node ID + fingerprint, enforce expiry, transition only that offer, and clear transient certificate/fingerprint material.
- [ ] Run all `fleet_enroll` tests and confirm the focused test passes.

### Task 2: Derive and forward authenticated peer fingerprint after report acceptance

**Files:**
- Modify: `src/fleet_hub.rs`
- Test: `src/fleet_hub.rs` unit tests

**Interfaces:**
- Produces: `Identity.fingerprint: String` derived from the peer leaf DER.
- Produces: `HubStore::report_accepted(&self, identity: &Identity)` with a default no-op.
- `handle` accepts a `HubStore` and invokes the callback only after `Hub::accept` succeeds.

- [x] Add tests for fingerprint derivation and callback behavior on accepted versus rejected reports.
- [x] Run focused Hub tests and verify the new assertions fail before implementation.
- [x] Derive the SHA-256 fingerprint in `peer_identity` from the authenticated DER.
- [x] Add the default callback and call it only after releasing the Hub mutex and after `accept` returns `Ok(())`.
- [x] Adapt `serve` and existing test stores to the generic HubStore interface.
- [x] Run all `fleet_hub` tests and confirm accepted/rejected behavior.

### Task 3: Connect Ctx report acceptance to Enroller

**Files:**
- Modify: `src/agent.rs`
- Test: `src/agent.rs` unit tests

**Interfaces:**
- `Ctx`'s `HubStore::report_accepted` calls `Enroller::acknowledge_first_report(identity.node_id, identity.fingerprint)`.

- [x] Add a test with an `AwaitingReport` offer proving only its exact identity transitions to `Enrolled` through the Ctx callback.
- [x] Run the focused test and verify it fails before implementation.
- [x] Implement the callback with lock acquisition kept separate from Hub report acceptance.
- [x] Run focused tests, `cargo fmt --all -- --check`, and `git diff --check`.

### Task 4: Whole-slice verification and checkpoint

**Files:**
- No product code unless verification finds a defect.
- Update: Issue #2 checkpoint after verified material progress.

- [x] Run focused tests for `fleet_enroll`, `fleet_hub`, and `agent`.
- [ ] Run the full Rust test suite on Linux CI (local macOS is not authoritative for Linux-only code paths).
- [ ] Run Clippy/release build through CI on the exact SHA.
- [x] Inspect diff and verify no host operations or unrelated changes.
- [ ] Commit and push the feature branch; open a Draft PR to trigger pull_request CI, keep it Draft until all checks on the exact SHA are green.
