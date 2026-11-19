# Fleet Join → First Report Correlation Design

## Intent

Complete the Hub-side trust transition that is already modeled by ADR-0051: a Join transport ACK means only that the Member claims it persisted the public trust bundle; the Enroller may become `Enrolled` only after the Hub accepts the first mandatory-mTLS report from the exact certificate delivered for that offer. This slice supplies the certificate-bound correlation primitive and connects the existing Report listener to it. It does not yet expose or start a per-offer Join listener, implement Member persistence, or claim end-to-end acceptance.

## Goals

- Bind `AwaitingReport` to the SHA-256 fingerprint of the exact issued fleet-node leaf certificate.
- Derive the reporting fingerprint from the authenticated TLS peer certificate, never from report JSON.
- Notify the Enroller only after `fleet::Hub::accept` has accepted the report.
- Transition only the matching node + exact certificate fingerprint to `Enrolled`.
- Keep report ingestion operational for existing tests and HubStore implementations that do not use enrollment.

## Non-goals

- No new public API, listener port, Hub configuration, UI, or Member-side persistence/report orchestration in this slice.
- No certificate signing changes; the root Fleet helper remains the signer.
- No real-host, firewall, route, sysctl, `CAP_NET_ADMIN`, merge, release, or deployment work.
- No two-host LAB claim.

## Approaches

1. **Node-ID-only correlation:** rejected because an older or unrelated certificate for the same node could complete a pending offer.
2. **Certificate serial correlation:** viable because serials are CA-issued, but serial formatting/issuer scope must be normalized and is less directly tied to the delivered bytes.
3. **Exact leaf SHA-256 fingerprint:** selected. The Hub already has the authenticated leaf DER at the TLS boundary; hashing those bytes binds the report to the exact certificate delivered by Join and avoids trusting any body field.

## Design

`Enroller::mark_delivered` parses its own issued certificate after the Join transport ACK and derives the SHA-256 leaf fingerprint internally, so the caller cannot supply or spoof this binding. The offer stores it and moves to `AwaitingReport`. The Hub's `peer_identity` derives the same SHA-256 fingerprint from the authenticated TLS leaf. After `fleet::Hub::accept` returns `Ok(())`, the HubStore callback passes certificate-derived node ID and fingerprint to the Enroller. The Enroller moves to `Enrolled` only if one `AwaitingReport` offer matches both values, then clears the transient certificate/fingerprint material. Rejected reports never trigger the callback. The Ctx callback captures the timestamp only after the full report has been read and accepted, then passes it into Enroller so the 15-minute offer expiry also bounds the first-report transition. The HubStore callback is mandatory for each implementation; the bare Mutex<Hub> test adapter explicitly no-ops, while the runtime Ctx performs the Enroller correlation.

## Security constraints

- Never accept node ID, serial, or certificate fingerprint from the report body as identity evidence.
- Do not call the enrollment callback before the report's profile, schema, sequence, rate and inventory checks pass.
- A Join persistence ACK or first report arriving after the offer expiry must close, not enroll, the pending offer.
- Keep Hub mutex and Enroller mutex acquisition non-overlapping to avoid lock-order deadlocks.
- A wrong node, wrong certificate, duplicate/replayed report, rejected report, or report before delivery ACK must not enroll.

## Acceptance

Focused tests prove: delivery ACK is a prerequisite; wrong node and wrong fingerprint do not enroll; exact certificate fingerprint does; rejected reports do not trigger enrollment; accepted reports invoke the callback with identity derived from the TLS leaf; existing report listener behavior remains intact. Full Linux CI on the exact branch SHA is required. Two-host LAB remains a later gate after listener and Member orchestration are implemented.
