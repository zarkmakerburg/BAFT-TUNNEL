# Task 2 report — Fleet-specific offer and bounded message framing

Status: **PASS — implementation and exact-SHA CI verified**

Implementation HEAD: `5699c38849a714d657f09c0e708568df963de602`

Implemented:
- Fleet-only `BAFTFLEETJOIN1:` offer prefix with canonical base64url-no-padding JSON encoding.
- Version, host/TLS name, port, Hub and node IDs, exact 32-byte SHA-256 Hub pin, 8-byte offer ID, 128-bit token and nonce, and bounded 15-minute expiry validation.
- Strict `deny_unknown_fields` request, response and acknowledgement schemas. The offer intentionally has no CA/CRL fields; public Hub identity, CA and CRL are part of the successful Join response only.
- `JoinRequest` validates the offer/node/token/nonce and CSR size; `JoinResponse` requires all public certificate/trust fields on success and forbids certificate material on refusal.
- Length-prefixed JSON frames with a 64 KiB maximum, zero/oversized/truncated frame rejection, and 10-second total read/write deadlines. Oversized serialized writes are rejected before any bytes are sent.
- Secret-bearing offer/request structs do not implement `Debug`; message bodies are not logged.

Test-first evidence:
- Test-only commit `ec6e7255d90cf7dd586d54b0f282812f18862d69`, run [44508626433](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44508626433): focused Rust test step failed only because the expected Task 2 symbols/functions were not yet implemented (`JoinOffer`, `JoinRequest`, `JoinResponse`, `JoinAck`, `FLEET_JOIN_PREFIX`, `MAX_JOIN_FRAME`, `read_frame`, `write_frame`). Formatting passed; other workflow jobs passed. This is the intended RED result.
- Intermediate implementation SHA `3424b564c6d4c329a485181abf406761ce5113a9`: Rust tests passed, but Clippy identified one unused test import (`timeout`). The import was removed in `5699c38`; no functional code change was needed for that correction.

Exact-SHA CI evidence for `5699c38849a714d657f09c0e708568df963de602`:
- `phase1-rust`: [44512226284](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44512226284) — SUCCESS; all six jobs passed.
- `phase0`: [44512181372](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44512181372) — SUCCESS.
- Same-SHA package/signature verification: [44512181308](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44512181308) — SUCCESS.
- Rust tests: 430 passed, 0 failed, 5 ignored; an additional test binary reported 6 passed. Clippy, formatting and release build passed.
- Local `cargo fmt --all -- --check` and `git diff --check` passed.

Limitations:
- No one-shot Hub Join listener or member-side network session yet.
- No Enroller integration, trust-bundle persistence, first mTLS Report/ACK integration, or two-host LAB acceptance.
- Join remains unadvertised and cannot mark a node Enrolled.
