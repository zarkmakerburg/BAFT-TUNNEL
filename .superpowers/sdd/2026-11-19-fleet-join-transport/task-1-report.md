# Task 1 report — Fleet-specific TLS identity primitives

Status: **PASS — implementation and exact-SHA CI verified**

Implementation HEAD: `532f4462aca8245a9973fc6437a6cc6b9a3cd7ab`

Implemented:
- TLS 1.3-only Fleet Join server acceptor with no client certificate requirement and early data disabled.
- Separate client connector whose bootstrap trust anchor is the exact SHA-256 fingerprint of the Hub leaf supplied by the trusted offer.
- Leaf-only certificate presentation; intermediates are rejected.
- Hub identity checks before application data: certificate validity, exact CN/hub ID, CA:false, digitalSignature-only key usage, serverAuth plus Fleet Hub EKU, exact Fleet URI SAN, and exact DNS/IP SAN for the dial target.
- TLS 1.3 handshake signatures are verified using rustls ring algorithms. TLS 1.2 is rejected.
- CA/CRL are not required before Join; they are intended to be delivered only after the pinned TLS session is established.
- Existing persistent mandatory-mTLS Report listener configuration is unchanged.

Focused test coverage includes pinned-leaf success without preloaded CA/CRL, wrong pin, wrong Hub ID/URI, wrong DNS SAN, exact and wrong IP SAN, expired/not-yet-valid certificates, and wrong Fleet Hub EKU.

Exact-SHA CI evidence for `532f4462aca8245a9973fc6437a6cc6b9a3cd7ab`:
- `phase1-rust`: [44488028597](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44488028597) — SUCCESS. Rust focused tests, Clippy, formatting, release build, Ubuntu 22.04 baseline/systemd, Ubuntu 24.04 installer, React panel, UI Playwright and offline CA container jobs all passed.
- `phase0`: [44488007805](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44488007805) — SUCCESS.
- Same-SHA package/signature verification: [44488007895](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44488007895) — SUCCESS.
- Local `cargo fmt --all -- --check` and `git diff --check` passed at the latest implementation commit (recorded in progress ledger).

Limitations:
- No Join offer/framing, listener, member request/session, trust-bundle delivery, Enroller integration, or first-report ACK network path yet.
- No two-host LAB acceptance. Join remains unadvertised and cannot mark a node Enrolled.
- This report is not a claim of end-to-end Fleet Join acceptance.
