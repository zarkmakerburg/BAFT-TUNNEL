# ADR-0027: panel-managed offline tcp-tls certificate enrollment

**Status:** Proposed for D2 implementation; not yet accepted by runtime code or LAB
**Date:** 2026-11-10
**Related:** Issue #149, ADR-0012, ADR-0023, ADR-0026

## Context

The tcp-tls engine already has an offline PKI workflow in `scripts/pki/baft-pki.sh`: a node creates a P-256 private key and CSR locally, an offline CA validates the CSR and issues a leaf certificate with a BAFT node URI SAN and a configured DNS/IP SAN, and the operator distributes the current CA certificate and CRL. The current panel has no complete CSR/export/import workflow. The local Agent runs unprivileged as `baft-tunnel-agent`; it must not gain arbitrary root command execution or read the tunnel private key.

This decision adds a guided local-panel workflow without adding online enrollment or moving the CA private key online.

## Verified current implementation baseline

Source review on `main` at `98a39464d3ed3954acbf666b602c5f42154c9a29` confirms:

- `setup::tls_material()` expects exactly `node.key`, `node.crt`, `ca.crt`, and `ca.crl` in the supplied setup directory; the current installed layout uses `/etc/baft-tunnel`.
- The runtime config stores explicit `ca`, `crl`, `cert`, and `key` paths; credential import must not silently rewrite these fields or assume a hot reload.
- `setup::adopt_for_service()` changes `node.key` to `baft-tunnel:baft-tunnel` while preserving 0600, and gives public PKI files group-readable permissions for the tunnel service.
- The Agent runs as `baft-tunnel-agent`, has `/etc/baft-tunnel` read-only, and may write only its state directory plus `/run/baft-tunnel/control`.
- The existing root setup helper is a fixed systemd path-unit command, accepts only init/join today, and is sandboxed with write access to `/etc/baft-tunnel` and the control directory.

**Implication:** the first implementation must preserve the existing flat PKI filenames and single-node credential set under `/etc/baft-tunnel`. Do not introduce a per-instance PKI subdirectory or multi-instance credential isolation without a separate migration/architecture decision. Extend the existing helper using a closed operation enum and bounded request/receipt protocol; do not grant the Agent direct write access to `/etc/baft-tunnel`. The helper must create private key files with final `baft-tunnel:baft-tunnel` ownership and 0600 mode, and public files with the existing group-readable policy.

## Decision

### 1. Trust and privilege boundaries

- The browser communicates only with the existing loopback-only, bearer-token protected Agent API. No CORS, remote listener, or new authentication model is introduced.
- The Agent validates requests and submits a bounded, instance-scoped request to a fixed root helper operation. The helper accepts a closed enum of operations and fixed instance paths only; it does not accept executable names, shell fragments, arbitrary filesystem paths, or caller-selected ownership/mode.
- The browser and Agent may handle public CSR/certificate/CA/CRL bytes, but must never receive the node private key.
- The offline CA private key remains offline. Signing remains the existing operator-controlled `issue-node` operation.

### 2. Host-side CSR generation

- Generate an EC P-256 key on the target node using OpenSSL through a fixed helper operation (no shell interpolation).
- Bind the CSR subject CN to the instance's configured node ID. The helper must independently validate the node ID and generated CSR.
- Store the private key under the existing fixed `/etc/baft-tunnel` PKI layout with restrictive ownership and mode (target tunnel service identity, private key 0600; do not create a parallel directory). Never overwrite an existing key or CSR. A second generation request returns a conflict and requires an explicit, separately designed rotation workflow.
- Return only the PEM CSR, a SHA-256 public-key fingerprint, node ID, expected DNS/IP SAN and a non-secret operation receipt.
- Persist the CSR alongside the key so an interrupted panel session can retrieve the same CSR. CSR retrieval is read-only and never returns key bytes.

### 3. Offline signing handoff

- The operator downloads/copies the CSR to the offline CA and uses the existing `baft-pki.sh issue-node` command. The panel must clearly state that CSR signing is an offline/manual step and must not imply the panel signs certificates.
- The signed package consists of leaf certificate, CA certificate and current CRL. The private key is not part of the package.

### 4. Certificate import and validation

Before activation, the privileged helper validates all inputs using OpenSSL and fixed paths:
1. PEM parsing succeeds; each input has a strict byte limit and exactly the expected object type.
2. Leaf public key matches the locally stored private key.
3. Leaf chain validates to the uploaded CA certificate; the CRL is signed by that CA, current, and passes revocation checking.
4. Leaf is not a CA, has clientAuth and serverAuth EKUs and digitalSignature key usage.
5. Exactly one URI SAN equals `urn:baft-tunnel:node:<configured-node-id>`.
6. Exactly one DNS or IP SAN equals the configured server name/address; no wildcard or extra identity SAN is accepted.
7. Validity dates are current and within the PKI policy.
8. The certificate/CA/CRL set is mutually consistent. No uploaded CA private key is accepted or stored.

Validation failure leaves the active credential set unchanged. Successful import writes a new versioned candidate directory, fsyncs files and directory, sets fixed ownership/modes, and atomically switches a fixed `current` reference only after all checks pass. A previous valid set is retained for rollback. Symlinks, non-regular files, unexpected files, path traversal, replayed operation IDs and partial candidate sets are rejected.

### 5. Runtime activation boundary

- Importing credentials is not the same as applying or starting a tunnel. The panel must show the installed fingerprint and an explicit next step.
- Existing configuration paths and runtime reload semantics must be inspected before wiring activation. If the current tcp-tls config pins CA/CRL/certificate/key paths directly, implementation must preserve compatibility and use a reviewed atomic path strategy; do not silently rewrite active config or restart services.
- The existing private key must remain unreadable to the Agent. The tunnel service remains the only runtime consumer.
- Do not expose tcp-tls as wizard-ready until both nodes have valid credentials and all preflight/capability contracts accurately represent the lifecycle.

### 6. API and UI contract

Proposed API routes (names provisional until implementation):
- `POST /v1/pki/csr`: create once; conflict if key/CSR already exists.
- `GET /v1/pki/csr`: retrieve the existing public CSR and metadata.
- `POST /v1/pki/certificate`: bounded multipart or JSON-encoded PEM import; no arbitrary path fields.
- `GET /v1/pki/status`: report only non-secret state, fingerprints, validity and validation status.

Panel flow:
1. Generate or retrieve CSR.
2. Download/copy CSR and show the exact offline CA signing command.
3. Upload leaf certificate, CA certificate and CRL.
4. Show validation results and installed fingerprint.
5. Clearly separate credential installation from tunnel Apply/Start.

All strings are added to both Persian RTL and English locales. DOM updates continue using the existing safe DOM helpers; no `innerHTML), inline scripts, CDN, or new frontend build chain.

### 7. Audit and receipts

Audit only actor fingerprint, instance, operation ID, action, outcome, certificate fingerprints, validity window and validation failure category. Never log PEM bodies, private keys, API tokens, or bundle contents. Receipts contain no secrets and are idempotent.

## Required implementation slices

| Slice | Work | Evidence |
|---|---|---|
| D2-A | Reconcile actual config credential paths, systemd ownership and fixed helper design | Reviewed source map + tests |
| D2-B | Add fixed helper request/receipt operations for CSR create/read | Key custody, no-overwrite, replay and permission tests |
| D2-C | Add Agent API routes with strict auth, body limits, setup-mode behavior and audit | Rust route tests incl. unauthenticated, malformed and oversized requests |
| D2-D | Add certificate/CA/CRL validation and atomic versioned activation | Positive and negative OpenSSL tests, failure-injection rollback |
| D2-E | Add Persian/English panel flow and safe CSR download/upload | Browser tests and accessibility checks |
| D2-F | Exact-head Rust, lint, panel and systemd CI; then two-host LAB using the candidate build | CI run and real-host LAB evidence tied to exact SHA |

## Explicit non-goals

- Online enrollment, CA signing from the panel, remote panel access, multi-user RBAC, automatic CA/pin rollover, or production deployment.
- Exporting, displaying, logging, or transmitting node private keys.
- Replacing active credentials or restarting services as a side effect of certificate upload.
- Enabling tcp-tls in capabilities/wizard before the full credential lifecycle is implemented and accepted.

## Acceptance blockers

D2 remains **OPEN / NOT VERIFIED** until implementation, negative tests, exact-head CI, and two-host LAB evidence all pass. This ADR is a design artifact only; it is not evidence that any route, UI, helper, or credential operation exists.
