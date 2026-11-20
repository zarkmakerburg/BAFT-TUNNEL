# Fleet PKI DNS Name Constraint Compatibility Design

Status: selected for implementation in the Fleet Join stack; no merge, release, deployment or real-host operation is authorized by this design.

## Intent

Make Fleet Hub and Member certificates usable by the project's existing rustls 0.23 / rustls-webpki 0.103.16 verification path without weakening certificate-chain, EKU, CRL or application identity checks.

## Current behavior and confirmed incompatibility

`src/fleet_ca.rs` creates a critical CA Name Constraints extension with `permitted;URI:.fleet.baft.invalid`. Fleet node leaves carry only `baft-fleet://<node>.fleet.baft.invalid`; Hub leaves carry the corresponding `baft-fleet-hub://...` URI plus the external dial DNS/IP SAN.

Both persistent mTLS roles use `WebPkiServerVerifier` / `WebPkiClientVerifier` in `src/tls.rs`. The locked `rustls-webpki 0.103.16` implementation explicitly rejects URI name constraints fail-closed (`src/subject_name/mod.rs`, `GeneralName::UniformResourceIdentifier`). Consequently, a valid Fleet chain containing these URI SANs cannot pass the standard verifier. Application-level URI checks happen after the TLS handshake and cannot repair this chain-validation failure.

## Goals

- Retain a critical, CA-enforced namespace restriction for all Fleet leaf certificates.
- Keep the Fleet URI SAN as the canonical application identity and continue exact URI, EKU and fingerprint checks.
- Use a name-constraint form supported by the existing rustls-webpki version.
- Keep the TCP dial address independent from the TLS certificate identity, for both bootstrap Join and persistent Report.
- Preserve mandatory CRL checking and fail-closed behavior; add no dependency and no custom verifier.

## Non-goals

- No removal or downgrade of Name Constraints.
- No custom rustls verifier, OpenSSL runtime TLS, or new cryptographic dependency.
- No change to Fleet URI schemes, enrollment state transitions or report authorization semantics.
- No `CAP_NET_ADMIN`, firewall, route, sysctl, SSH, Go-core, real-host, release or deployment changes.

## Approaches considered

1. **DNS-constrained identity SANs (selected).** Set the Fleet CA critical permitted subtree to `DNS:.fleet.baft.invalid`; issue every node and Hub leaf with exactly one DNS identity SAN `<id>.fleet.baft.invalid` and its existing exact URI SAN. Keep URI as the application identity, and require the DNS SAN to encode the same ID. For Hub connections, use that internal DNS identity as TLS `hub_name` / SNI while retaining the real dial endpoint in `host` / `hub_addr`. This uses the existing standard WebPKI verifier and preserves CA namespace enforcement.
2. **Custom URI-aware rustls verifier.** Keep the current URI constraint and implement a custom verifier that validates chain signatures, validity, EKU, CRL and URI name constraints. Rejected for this fix: it duplicates high-risk PKI logic and creates a long-lived custom verification surface where standard WebPKI already provides the required chain and revocation behavior.
3. **Remove or make URI Name Constraints non-critical.** Rejected: it removes the CA-level namespace guard or changes failure semantics and is not an acceptable compatibility workaround.

## Decision and contracts

- Fleet CA: `nameConstraints=critical,permitted;DNS:.fleet.baft.invalid`.
- Node leaf SANs: exactly one `DNS:<node-id>.fleet.baft.invalid` and exactly one `URI:baft-fleet://<node-id>.fleet.baft.invalid`.
- Hub leaf SANs: exactly one `DNS:<hub-id>.fleet.baft.invalid` and exactly one `URI:baft-fleet-hub://<hub-id>.fleet.baft.invalid`. Do not place the external dial hostname or IP in the Fleet Hub certificate.
- The DNS SAN is the PKI-constrained transport identity. The URI SAN remains the canonical Fleet application identity. Runtime validation must reject missing, duplicate, extra, malformed or mismatched identity SANs.
- `JoinOffer.host` / `MemberConfig.hub_addr` carry the real TCP destination. `JoinOffer.hub_name` / `MemberConfig.hub_name` carry the exact internal DNS identity derived from `hub_id`; they are not the dial address. `advertise_host` remains the real externally reachable address.
- `JoinOffer` and Hub config validation must enforce `hub_name == <hub_id>.fleet.baft.invalid`; do not trust arbitrary TLS names supplied by an offer.
- Standard WebPKI continues to enforce chain, validity, DNS name constraints, server name and CRL. Existing application checks continue to enforce exact URI identity and Fleet EKU.

## Compatibility and migration

A CA certificate containing the old URI Name Constraints and leaves issued under it cannot be reused with the new standard WebPKI path. Before any real-host activation, inspect whether a Fleet CA or leaves were initialized; if so, require a separately authorized reinitialization/re-enrollment plan. No host state is changed by this work. Fleet Join remains Preview/unadvertised until its full acceptance gates pass.

## Acceptance evidence

- CA and leaf profile tests assert the exact critical DNS permitted subtree and exact paired DNS/URI SANs.
- Actual `WebPkiServerVerifier` and `WebPkiClientVerifier` integration tests accept valid Fleet Hub/Member chains under the DNS constraint.
- Negative tests reject out-of-namespace DNS SANs, missing/duplicate/mismatched DNS or URI SANs, wrong Hub TLS name, wrong EKU and revoked/expired CRL as applicable.
- Hub TCP destination remains independent of SNI/TLS identity in Join and Report tests.
- Exact-head Linux CI is required. Local macOS-only or mocked tests are not a substitute for hosted Linux evidence; no real two-host LAB is claimed.

## Implementation checkpoint — 2026-11-20

Implemented locally through commit `99a06e0ef9f138a74accb027e6a312a2b886f698`. The Fleet CA uses critical DNS Name Constraints; Member and Hub certificates have exact paired DNS/URI SANs; Hub dial address and TLS identity are separated; runtime checks reject duplicate/mismatched SAN identities; standard WebPKI and mandatory CRL verification remain in place.

Local macOS evidence: focused Fleet suite `126 passed, 0 failed`; Hub paired-identity test `1 passed`; formatting and diff checks passed. A controlled baseline run with the old URI-only Name Constraints failed the actual WebPKI mTLS handshake (`AlertReceived(CertificateUnknown)`), confirming the compatibility regression and fix. The baseline-only source edit and compatibility shims were restored byte-for-byte. The macOS test run required temporary compatibility shims for a Linux-only `prctl` call and the hard-coded `/usr/bin/openssl` path; both source files were restored byte-for-byte. A direct unshimmed macOS build remains blocked by the existing `libc::prctl` symbols.

Acceptance is not complete: exact-head Linux CI has not run for this commit, no real two-host LAB was performed, and the branch has not been pushed. Existing CA/leaf material created with URI Name Constraints requires a separately authorized migration/re-enrollment plan before real-host activation.
