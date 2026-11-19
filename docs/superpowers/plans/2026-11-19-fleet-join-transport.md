# Fleet Join TLS Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an isolated Fleet-specific TLS 1.3 server-authenticated Join transport and pinned member client, without enabling or advertising enrollment before later Enroller/Report integration and two-host acceptance.

**Architecture:** A temporary, per-offer Hub listener uses TLS 1.3 with no client certificate, separate from the existing persistent mandatory-mTLS Report listener. The member verifies the Hub through the Fleet CA/CRL, exact leaf SHA-256 pin, Hub EKU/URI identity and dial-target DNS/IP SAN before sending a Fleet-specific bounded request. The server keeps the TLS session open through operator approval and certificate/trust delivery; the member acknowledges only after local installation and its caller's first-report step. This transport slice does not mark a node Enrolled.

**Tech Stack:** Rust 2024, Tokio, rustls 0.23, tokio-rustls 0.26, serde/serde_json, SHA-256, existing x509-parser and OpenSSL helpers. No new dependency.

**Spec:** Owner-approved 2026-11-19 Addendum in [PR #399](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/399/files), `docs/ADR-0051-add-server-from-panel.md`, sections “Fleet Join TLS bootstrap and certificate delivery” and “Required negative and failure tests”. The base code for this stacked implementation is PR #402 exact HEAD `337901e18f47f1c54a0f17a07704cefd8f273b36`.

## Global Constraints

- TLS 1.3 only; Join is server-authentication-only and Report remains mandatory mTLS.
- Never reuse tunnel pairing message types, codes, identities, or state.
- Hub identity pin is the SHA-256 fingerprint of the exact Hub leaf certificate; also validate chain, CRL, validity, Fleet Hub EKU, URI SAN and dial-target DNS/IP SAN.
- One-time token is 128 bits minimum; token and nonce never appear in URLs, logs, or audit.
- CSR private key is generated and retained by the member; only the CSR crosses Join.
- Frames, fields, concurrency, failures, and read/write deadlines are bounded; fail closed on malformed or oversized input.
- Only public member certificate, CA certificate and CRL may be delivered. No CA or member private key is transmitted.
- No firewall, route, sysctl, host-network, or `CAP_NET_ADMIN` changes. No real-host mutation, merge, release, or deploy.
- This slice remains unadvertised and cannot claim Enrolled; Enroller and accepted first-report integration are later slices.

## Review Focus

- A wrong or stale Hub pin must fail before the member sends its CSR.
- A valid CA chain with the wrong Hub EKU, URI identity, or dial-target SAN must fail closed.
- Slow, oversized, truncated, replayed, and malformed frames must not block or exhaust the listener.
- A failed delivery or missing member acknowledgement must not be reported as success.
- The member's private key and Fleet CA private key must never enter a Join message, browser response, argv, environment, or log.

---

### Task 1: Fleet-specific TLS identity primitives

**Files:**
- Modify: `src/tls.rs`
- Create: `src/fleet_join.rs`
- Modify: `src/main.rs`
- Test: `src/fleet_join.rs` unit tests

**Interfaces:**
- `tls::fleet_join_acceptor(cert, key) -> Result<TlsAcceptor>` builds TLS 1.3 with no client auth and early data disabled.
- `fleet_join::HubPinVerifier` combines WebPKI chain/CRL/name/validity checks with an exact leaf pin; the Fleet-specific identity check runs after handshake.
- `fleet_join::client_connector(ca, crl, pin) -> Result<TlsConnector>` wraps `WebPkiServerVerifier` with an exact leaf SHA-256 pin check; chain, CRL, validity and dial-target DNS/IP SAN are verified by WebPKI.
- After handshake, `fleet_report::verify_hub_identity(leaf_der, hub_id)` (or a Fleet Join-specific strict wrapper) enforces the Fleet Hub EKU and URI SAN before any request bytes are sent.

- [ ] Write tests proving TLS 1.3 server-only acceptor construction and rejecting wrong pin, invalid CA chain, expired certificate, wrong EKU, wrong URI SAN, and wrong DNS/IP SAN.
- [ ] Run the focused tests and confirm they fail for the intended missing behavior.
- [ ] Implement the minimal TLS acceptor and pinned verifier; keep signature verification delegated to rustls WebPKI/ring.
- [ ] Run focused tests and `cargo fmt --all -- --check`.

### Task 2: Fleet-specific offer and bounded message framing

**Files:**
- Modify: `src/fleet_join.rs`
- Test: `src/fleet_join.rs` unit tests

**Interfaces:**
- `JoinOffer { v, host, port, hub_name, hub_id, hub_pin, offer_id, node_id, token, nonce, expires }` has `encode/decode/validate` using a Fleet-only prefix and bounded base64url JSON; no `PairCode` reuse.
- `JoinRequest { offer_id, node_id, token, nonce, csr_pem }` and `JoinResponse { ok, certificate_pem, ca_pem, crl_pem, reason }` are strict (`deny_unknown_fields`); response contains public material only.
- `JoinAck { ok }` confirms transport delivery only; it does not imply Enrolled.
- Length-prefixed JSON frames use a fixed maximum and bounded read/write deadlines.

- [ ] Write tests for offer encoding, invalid fields, unknown JSON fields, frame lengths 0/oversized/truncated, and timeout.
- [ ] Run focused tests and confirm the missing cases fail.
- [ ] Implement strict Fleet-only types and framing; never log request or response bodies.
- [ ] Run focused tests and formatting.

### Task 3: One-shot Hub Join listener and member session

**Files:**
- Modify: `src/fleet_join.rs`
- Test: `src/fleet_join.rs` async integration tests

**Interfaces:**
- `Admission::{Retry, Close, Deliver(JoinResponse)}` lets the Enroller callback distinguish a retryable bad token from a consumed/closed offer and an operator-approved delivery.
- `serve<F, Fut>(listener, acceptor, offer_id, node_id, expires, decide)` handles one offer, bounded concurrent connections and per-IP/total failures; it invokes `decide(JoinRequest, peer_addr)` and holds the TLS session through approval and acknowledgement.
- `join(offer, csr_pem)` verifies the exact Hub pin and identity before sending a request; waits no longer than the offer expiry.
- `JoinSession::ack(ok)` writes the final bounded acknowledgement over the same pinned TLS session.
- The listener returns success only after the member acknowledgement; integration with the Report listener and Enroller state transition is deliberately deferred to the next slice.

- [ ] Write a loopback TLS integration test with a real generated Hub certificate and a valid member CSR.
- [ ] Add negative tests for bad pin, Hub identity mismatch, wrong offer/node, wrong token/nonce, expired offer, concurrent/replayed submission, oversized/slow frame, disconnect before response, and missing acknowledgement.
- [ ] Run tests and confirm expected failures before implementation.
- [ ] Implement the one-shot accept loop, bounded session lifecycle and member client.
- [ ] Verify success, refusal, expiry, disconnect and timeout behavior with exact test output.

### Task 4: Module registration, documentation and handoff

**Files:**
- Modify: `src/main.rs`
- Modify: `docs/ADR-0051-add-server-from-panel.md` only if a precise implementation note is needed; do not copy the unmerged PR #399 addendum into this branch.
- Test: full hosted `phase1-rust` workflow on the exact new HEAD

- [ ] Register `mod fleet_join;` and ensure no public API or UI advertises Join yet.
- [ ] Run `cargo fmt --all -- --check`, `git diff --check`, focused Rust tests, and inspect the complete diff.
- [ ] Commit the transport slice on this isolated branch and push only this task branch.
- [ ] Open a Draft PR stacked on PR #402; record exact SHA, CI, limitations and next Enroller/Report integration in Issue #2.

