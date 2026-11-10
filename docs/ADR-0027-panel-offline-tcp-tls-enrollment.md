# ADR-0027: panel-managed offline tcp-tls certificate enrollment

**Status:** D2-B1/B2/C/D/E implementation candidate on feature/d2-panel-tcp-tls-enrollment; local Rust and browser evidence is recorded below. D2-F (exact-head Linux CI and two-host LAB) remains OPEN / NOT VERIFIED. No merge, release or production deployment.
**Date:** 2026-11-10
**Related:** Issue #149, ADR-0012, ADR-0023, ADR-0026

## Context

The tcp-tls engine already has an offline PKI workflow in `scripts/pki/baft-pki.sh`: a node creates a P-256 private key and CSR locally, an offline CA validates the CSR and issues a leaf certificate with a BAFT node URI SAN and a configured DNS/IP SAN, and the operator distributes the current CA certificate and CRL. The current panel has no complete CSR/export/import workflow. The local Agent runs unprivileged as `baft-tunnel-agent`; it must not gain arbitrary root command execution or read the tunnel private key.

This decision adds a guided local-panel workflow without adding online enrollment or moving the CA private key online.

## Verified current implementation baseline

Source review on `main` at `98a39464d3ed3954acbf666b602c5f42154c9a29` confirms:

- setup::tls_material() requires node.key, node.crt, ca.crt, and ca.crl in the supplied setup directory; the current installed layout uses /etc/baft-tunnel. The additional non-secret node.csr.meta file is not consumed by runtime TLS setup.
- The runtime config stores explicit `ca`, `crl`, `cert`, and `key` paths; credential import must not silently rewrite these fields or assume a hot reload.
- `setup::adopt_for_service()` changes `node.key` to `baft-tunnel:baft-tunnel` while preserving 0600, and gives public PKI files group-readable permissions for the tunnel service.
- The Agent runs as `baft-tunnel-agent`, has `/etc/baft-tunnel` read-only, and may write only its state directory plus `/run/baft-tunnel/control`.
- The existing root setup helper is a fixed systemd path-unit command, accepts only init/join today, and is sandboxed with write access to `/etc/baft-tunnel` and the control directory.

**Implementation amendment (D2-D):** existing installations keep and continue to use the legacy flat node.crt, ca.crt, ca.crl, and node.key paths. New panel-managed enrollment keeps node.key at the fixed /etc/baft-tunnel/node.key path, but stores each immutable public credential set under /etc/baft-tunnel/pki/sets/<sha256>/ and switches /etc/baft-tunnel/pki/current atomically. New tcp-tls config points at pki/current/{node.crt,ca.crt,ca.crl}; legacy configs are not rewritten. This narrowly extends the original flat-file contract because three separate flat-file replacements cannot be made atomic as one credential set. It does not create per-instance PKI directories or multi-instance credential isolation. The root helper still uses a closed operation enum and bounded request/receipt protocol; the Agent never gets write access to /etc/baft-tunnel. node.key remains baft-tunnel:baft-tunnel, mode 0600; versioned public files are root-owned, group-readable and not group/world writable.

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
- Keep tcp-tls Init gated until this node has a valid installed credential set. The peer credential and identity are checked during Join/pairing; the UI must not label two-node service readiness as accepted before D2-F.

### 6. API and UI contract

Implemented local Agent API routes:
- POST /v1/pki/csr/create: confirmed, pre-Init create; conflicts if credentials already exist or the instance is configured.
- POST /v1/pki/csr/read: retrieve the existing public CSR and bound metadata in setup mode, or for the matching configured tcp-tls node.

Implemented locally:
- POST /v1/pki/certificate/install: bounded PEM import; no arbitrary path fields. The root helper validates the set, stores it under a content-addressed version, and atomically switches the pki/current pointer.
- The upload response contains only the active-set digest, certificate fingerprint/expiry, CRL remaining lifetime, and explicit service_restarted: false.
- A separate GET /v1/pki/status endpoint is deferred: after a panel reload, the operator can recover the same CSR and safely re-upload the same certificate set; the helper recognizes an identical active set idempotently. No status endpoint is required to gate Init in this slice.

Panel flow:
1. Enter this node's own DNS/IP certificate identity separately from the peer's certificate name.
2. Generate or retrieve CSR; show the public-key fingerprint and offer copy/download. The private key stays on the host.
3. Show the exact baft-pki.sh issue-node CA_DIR NODE_ID CSR_FILE OUTPUT_DIR SERVER_NAME command for the offline CA.
4. Accept only three public files (leaf certificate, CA certificate and CRL); reject private-key PEMs and oversized/wrong-type files in the browser, then revalidate everything in the root helper.
5. Show the installed certificate fingerprint, validity and CRL lifetime. Keep Init disabled until the upload receipt confirms a valid set. Upload never restarts the service.

All strings are added to both Persian RTL and English locales. DOM updates continue using the existing safe DOM helpers; no innerHTML, inline scripts, CDN, or new frontend build chain.

### 7. Audit and receipts

Audit only actor fingerprint, instance, operation ID, action, outcome, certificate fingerprints, validity window and validation failure category. Never log PEM bodies, private keys, API tokens, or bundle contents. Receipts contain no secrets and are idempotent.


## Source-level integration map (reviewed on main)

The D2 implementation must account for these concrete seams:

- src/apply.rs::Stage remains a lifecycle enum. The separate src/pki_helper.rs::PkiOperation handles CSR create/read and certificate-set installation; it is not mixed into setup lifecycle stages.
- The setup helper continues to consume NAME.setup and write NAME.setup-result. The PKI helper uses the distinct NAME.pki / NAME.pki-result protocol and /var/lib/baft-tunnel/pki-receipts.
- systemd/control/baft-tunnel-control-pki@.service preserves the setup helper's sandbox shape and adds only the fixed PKI receipt directory to its existing /etc/baft-tunnel, /var/lib/baft-tunnel, and control-directory write paths.
- scripts/install-agent-control.sh registers the PKI path/service templates as a separate pki operation; scripts/test-install.sh verifies staged installation and fixed unit command/path/permissions.
- src/agent.rs::route() has an explicit global path allowlist followed by Bearer authentication, then a separate configured-mode method/path match. setup_mode_route() has its own allowlist. D2 routes (POST /v1/pki/csr/create, /v1/pki/csr/read, /v1/pki/certificate/install) are explicitly present in both allowlists with tested authorization, setup/configured-mode and refusal behavior; no wildcard route.
- src/setup.rs::tls_material() is called during tcp-tls setup Init and requires the node key plus one valid certificate/CA/CRL set. It now resolves a validated pki/current versioned set for new D2 enrollment, while retaining the legacy flat-file layout for existing installations.
- src/setup.rs::plan() points new D2 configs at pki/current/ca.crt, pki/current/ca.crl, pki/current/node.crt, and the unchanged flat node.key. Existing flat configurations are not silently rewritten. A focused setup test verifies versioned path selection without flat-file migration.
- The PKI script new-node NODE_ID NODE_DIR already produces a private key and public CSR without overwriting existing material; issue-node on the offline CA checks the requested identity and SAN; verify-node verifies the issued certificate. Reuse or port these exact policies rather than creating a second, weaker PKI policy.

### Current implementation snapshot (local Mac; 2026-11-10)

- D2-B1/B2: separate root-helper CSR create/read protocol, digest-bound replay receipts, fixed systemd path/service units, and staged registration.
- D2-C: atomic SystemPkiClient requests, root-owned receipts, fixed config-dir guard, authenticated Agent routes, setup/configured-mode restrictions and secret-free audit.
- D2-D: strict PEM/identity/key/CA/EKU/KeyUsage/SAN/CRL validation; same-CA monotonic CRL updates; immutable content-addressed sets; protected directory/file ownership and modes; atomic pointer publication and rollback; no automatic restart; legacy PKCS#8 P-256 key normalization.
- D2-E: Persian/English Wizard UI for local SAN entry, CSR create/recovery/copy/download, offline CA instructions, three public PEM uploads, private-key rejection, install receipt and Init gating. Preflight treats a pristine node or a node with a complete pending CSR as an enrollment warning, but continues to block partial credential state. Browser QA uses mocked API responses and does not claim real root-helper/LAB behavior.
- Local evidence: focused certificate-install and active-set tests pass; the Persian/English Playwright flow passes 34 assertions including private-key file rejection and Init gating. Exact-head Linux CI and real-host LAB are still required. The prior serial full-suite host/platform baseline failures remain separate and must be rechecked on Linux.
- D2-F remains open. No Stable release or production readiness is claimed.

### Revised implementation order

1. D2-A (source map): complete; source paths and privilege boundaries above are recorded. This is review evidence only, not a runtime feature.
2. D2-B1/B2: local implementation candidate complete; target Linux/CI evidence remains open.
3. D2-C: local implementation candidate complete; exact-head CI and target Linux/LAB remain open.
4. D2-D: strict certificate/CA/CRL validation, immutable versioned sets and failure-safe atomic activation; legacy flat configs remain supported.
5. D2-E: Persian/English panel enrollment, CSR recovery/download and public certificate upload; Init is gated on a successful install receipt.
6. D2-F: exact-head CI, Linux/systemd acceptance and two-host LAB using the candidate SHA. Keep the stable-release gate closed until all evidence passes.

## Required implementation slices

| Slice | Work | Evidence |
|---|---|---|
| D2-A | Reconcile actual config credential paths, systemd ownership and fixed helper design | Reviewed source map + tests |
| D2-B1/B2 | Fixed root helper CSR create/read protocol and systemd registration | Local Mac implementation; exact-head CI and Linux/LAB remain open |
| D2-C | Authenticated Agent create/read routes, atomic request client, root receipt checks, setup/configured guards and audit | Local Mac implementation; exact-head CI and Linux/LAB remain open |
| D2-D | Strict certificate/CA/CRL validation, immutable versioned sets, protected metadata and atomic activation | Positive/negative OpenSSL tests, monotonic CRL test, active-pointer preservation, no-restart API receipt |
| D2-E | Persian/English panel flow, CSR recovery/download and public certificate upload | Playwright browser test, key-file rejection, Init gate, no browser errors |
| D2-F | Exact-head Rust, lint, panel and systemd CI; then two-host LAB | CI run and real-host LAB evidence tied to exact SHA |

## Explicit non-goals

- Online enrollment, CA signing from the panel, remote panel access, multi-user RBAC, automatic CA/pin rollover, or production deployment.
- Exporting, displaying, logging, or transmitting node private keys.
- Replacing active credentials or restarting services as a side effect of certificate upload.
- Claiming stable two-node tcp-tls readiness or enabling Reverse before the complete credential lifecycle and D2-F acceptance.

## Acceptance blockers

D2 remains **OPEN / NOT VERIFIED** until exact-head CI, Linux/systemd validation, and two-host LAB evidence pass. D2-B1/B2/C/D/E are local implementation candidates; no target-host/LAB acceptance, merge, release or production deployment is claimed.
