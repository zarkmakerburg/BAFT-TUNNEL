# ADR-0012: offline Phase 1 certificate bootstrap

**Status:** Accepted for experimental Phase 1 tooling
**Date:** 2026-11-06

## Context

The TCP/TLS data plane requires each node to present a CA-issued leaf certificate with one exact node URI SAN, a DNS/IP SAN for TLS server-name verification, a current CRL, and an out-of-band SHA-256 leaf pin. The runtime does not yet include an online enrollment service or encrypted node-key loader.

## Decision

- Keep the root CA private key encrypted and offline. The PKI helper creates a P-384 CA key and a CA certificate with `pathlen:0`, `keyCertSign`, and `cRLSign`.
- Generate each node's EC P-256 private key on that node with mode `0600`; transfer only its CSR to the offline CA.
- The CA signs CSRs only after exact node-ID/CN and key-type validation. CSR-requested extensions are never copied. The CA supplies the URI SAN `urn:baft-tunnel:node:<node-id>` and the operator-selected DNS or IP SAN.
- Certificates are end-entity certificates for both TLS client and server authentication, with a bounded validity period.
- The CA helper issues and verifies CRLs and supports revocation. Both peers must receive the updated CRL and restart before the revocation is considered applied.
- The helper never copies the CA private key to a node bundle.

## Consequences and limitations

- This is an offline/manual bootstrap tool, not online enrollment or a production control plane.
- Node private keys are currently unencrypted PEM files. File mode `0600` and a dedicated service account are minimum controls only; encrypted node-key storage and OS keystore/HSM support remain open.
- CRL distribution and process restart are manual.
- Leaf fingerprint pinning requires coordinated certificate/config updates; overlapping pin sets and safe automated rollover are not implemented.
- The PKI regression test creates a temporary test CA and verifies CSR checks, URI/DNS/IP SAN issuance, permissions, CA-key isolation, revocation, and mismatch rejection.
