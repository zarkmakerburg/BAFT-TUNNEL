# SDD ledger — plan: docs/superpowers/plans/2026-11-19-fleet-join-transport.md

Base: `337901e18f47f1c54a0f17a07704cefd8f273b36` (PR #402 exact-SHA CI green).
Plan commit: `325e3f91ab77d0c384d3528aeb533a37b5580b7b`.
Spec: owner-approved ADR-0051 addendum in PR #399. Independent external review is not required by owner; author self-review and CI/LAB acceptance still apply.

## Pre-flight conflict scan

| Pair / task | Shared interface or files | Finding / ruling |
|---|---|---|
| Task 1 ↔ Task 2 | `src/fleet_join.rs` | Task 1 produces TLS verifier/acceptor primitives; Task 2 adds message types and framing in the same module. No contradictory signatures after removing `JoinOffer` from Task 1. |
| Task 1 ↔ Task 3 | `src/fleet_join.rs`, `src/tls.rs` | Task 3 consumes Task 1's `client_connector` and `fleet_join_acceptor`; exact contracts recorded in plan. |
| Task 2 ↔ Task 3 | `src/fleet_join.rs` | Task 3 consumes `JoinOffer`, `JoinRequest`, `JoinResponse`, `JoinAck`, frame helpers and `Admission` defined in Task 2. |
| Task 1 | `src/tls.rs`, `src/fleet_join.rs`, `src/main.rs` | Tests exercise TLS 1.3/no-client-auth and pin + WebPKI + Hub identity validation. Module registration only; no API/UI advertisement. Consistent. |
| Task 2 | `src/fleet_join.rs` | Strict schemas and framing tests cover malformed/oversized/truncated frames and deadlines. Consistent. |
| Task 3 | `src/fleet_join.rs` | One-shot listener/client with retry/close/deliver decisions and same-session ack; Enroller/Report integration explicitly deferred. Consistent with scope. |
| Task 4 | `src/main.rs`, docs | Register module, do not advertise, hosted exact-SHA CI and Draft PR. Consistent. |

Ruling: Transport-level Join acknowledgement is not evidence of Enrolled. Only the later integration slice may transition Enroller to Enrolled after the Hub independently accepts the first mTLS report and returns the Join acknowledgement — mandated by ADR-0051, no relaxation.

## Task ledger

- Task 1: **complete** — implementation and all three exact-SHA workflows passed on `532f4462aca8245a9973fc6437a6cc6b9a3cd7ab`; see `task-1-report.md`.
- Task 2: **complete** — strict offer/message schemas and bounded framing accepted on `5699c38849a714d657f09c0e708568df963de602`; all three exact-SHA workflows passed; see `task-2-report.md`.
- Task 3: pending.
- Task 4: pending.

Task 1 RED attempt:
- Test-only commit: `46994f81048b2f75a402fd74fb069e75a3a103b1` (`test(fleet): specify pinned Join TLS identity`).
- Hosted `phase1-rust` run `44483997266` is still in progress; Rust job is still installing pinned cargo-audit, so no RED verdict is claimed yet.
- Local `cargo fmt --all -- --check` and `git diff --check` passed. No product code has been changed in Task 1.

Task 1 RED evidence:
- Hosted `phase1-rust` run `44483997266`, exact SHA `46994f81048b2f75a402fd74fb069e75a3a103b1`.
- Rust job reached the focused test command and failed at the intended missing API boundary: `E0425` for `tls::fleet_join_acceptor` and `fleet_join::client_connector`; no unrelated compile or fixture error. Logs fetched from the completed Rust job; the workflow's separate Ubuntu 22.04 systemd job is still running.
- This is a compile-time RED for the absent transport primitives. Functional TLS negative tests remain to be proven once the primitives compile.

Task 1 implementation mutation:
- Added TLS 1.3 server-only Fleet Join acceptor with early data disabled; existing Report `server_config` and mandatory-mTLS path are unchanged.
- Added WebPKI server verifier using Fleet CA + mandatory CRL/expiry enforcement, wrapped by an exact SHA-256 Hub leaf pin verifier; signature verification delegates to WebPKI.
- Added loopback TLS tests for server-auth-only handshake/TLS 1.3, wrong pin, wrong CA, expired Hub certificate, wrong DNS SAN, and Fleet Hub EKU/URI identity mismatch.
- Fresh local static checks after mutation: `cargo fmt --all -- --check` PASS; `git diff --check` PASS. Local Rust execution remains unavailable on macOS due existing Linux-only `libc::prctl` references; next proof is hosted Linux CI.
- Task 1 implementation commit: `7433d2ed74f914d6ebb7ad000bd1a7d2bb795903` (`feat(fleet): add pinned Join TLS primitives`), pushed to the isolated branch.
- Exact-SHA hosted `phase1-rust` run `44485228925` is in progress. Task 1 is not marked complete until its Rust tests and required workflow jobs finish successfully.

Task 1 verification finding / repair:
- First implementation run `44485228925` on SHA `7433d2e` failed compile in both Rust and Ubuntu 22.04 baseline: `WebPkiServerVerifier::builder(...).build()` already returns `Arc<WebPkiServerVerifier>`; the helper incorrectly wrapped it in a second `Arc`.
- Fixed by removing the redundant `Arc::new` mapping; commit `2d644519ae9a15c991419f77265b6475edc1790d` pushed. `cargo fmt --all -- --check` and `git diff --check` pass. Fresh exact-SHA CI for `2d64451` is pending until the prior run completes.
- Repair commit `2d644519ae9a15c991419f77265b6475edc1790d` pushed; prior full workflow `44485228925` was cancelled after the compile error was diagnosed (the Rust job had not reached tests, and the Ubuntu baseline exposed the same compile failure).
- Fresh exact-SHA `phase1-rust` run `44485606528` is queued for `2d64451`; same-SHA phase0 push `44485521263` and package/signature push `44485521411` are in progress.

Task 1 CI finding on `2d64451`:
- Hosted `phase1-rust` run `44485606528` Rust test job failed with 415 passed / 1 failed / 5 ignored. Only failing test was the valid server-auth-only handshake; rustls reported `NameConstraintViolation`.
- Diagnosis: the Fleet CA constrains Hub DNS SANs to `.fleet.baft.invalid`; the initial fixtures used `localhost` / `other.local`. This also meant several negative tests could have passed for the wrong reason. All Hub fixtures now use an allowed `hub-*.fleet.baft.invalid` name; wrong-SAN uses a different allowed suffix member, and wrong-pin now asserts the exact pin-mismatch error. This isolates each test condition.
- The same-SHA phase0 and package/signature workflows passed. `panel-react`, `panel-ui`, `offline-ca-container`, and Ubuntu 24.04 passed. Ubuntu 22.04 systemd job was still running when this finding was recorded.
- The same run `44485606528` also failed its Ubuntu 22.04 systemd Fleet Hub provisioning step: after the root helper completed, the harness reported `systemd did not deliver fleet-hub-key`. All preceding Ubuntu 22.04 stages passed (release build, control-directory migration, baseline LoadCredential lifecycle, control helper, agent lifecycle, and panel pairing). This exact helper/systemd path had passed on PR #401's earlier exact-SHA run `44467626828`; the current Join change does not touch fleet_helper, unit templates or credential drop-ins. Treat as an unresolved CI failure, not a pass or yet-proven flake; one bounded retry on the corrected fixture SHA will determine reproducibility.
- Corrected test-fixture commit `db1b3f060ee7483500be5ebd46afec14e321ff8c` pushed. Exact-SHA `phase1-rust` retry run `44486690568` is queued/running; push-triggered phase0/package checks will also be polled. The pin negative now asserts the exact pin-mismatch error; wrong CA, wrong SAN, and expiry fixtures each retain valid Fleet CA name constraints.

Architecture correction from the exact owner-approved ADR-0051 addendum in PR #399:
- The addendum explicitly says the joining member has no Fleet CA/CRL before Join; it authenticates the Hub using the exact leaf fingerprint in the trusted offer, plus validity, Fleet Hub EKU, URI SAN bound to `hub_id`, and DNS/IP SAN bound to `hub_name`. CA certificate and CRL are sent only after the pinned TLS session exists. The Join response also carries the public Hub identity material.
- The earlier implementation attempt using WebPKI CA/CRL validation before Join was incompatible with this bootstrap order. The corresponding run `44486690568` was cancelled; no PR was opened and no Join flow was advertised.
- Reworked `HubPinVerifier` to use the pinned leaf as the bootstrap trust anchor, reject intermediates, enforce TLS 1.3 handshake signatures through rustls ring, and validate validity, CN, basicConstraints, keyUsage, standard `serverAuth`, Fleet Hub EKU, exact URI SAN, and exact DNS/IP SAN before the handshake can carry CSR bytes. CA/CRL are not inputs to the connector.
- Expanded tests to cover pin-only success without preloaded CA/CRL, wrong pin, wrong Hub CN/URI, wrong EKU, DNS/IP SAN mismatch, valid IP SAN, expired and not-yet-valid certificates. Plan and Task 1 brief corrected to match ADR-0051.

Corrected Task 1 implementation commit:
- Commit `ac868a61b38f8f09ac5e82c995e3593a084274f8` (`fix(fleet): bootstrap Join from pinned Hub leaf`) pushed to `feature/fleet-join-transport-20261119`.
- The current client connector accepts only `pin_hex`, `hub_id`, and `hub_name`; no CA/CRL inputs. The pinned-leaf verifier rejects intermediates, enforces exact SHA-256 pin, TLS 1.3 signature verification via ring, certificate validity, CN, CA:false, digitalSignature-only key usage, serverAuth + FleetHub EKU, exact URI SAN and exact DNS/IP SAN.
- Correct exact-SHA `phase1-rust` run `44487763058` has been dispatched; Task 1 remains pending until Rust tests and required jobs pass.

Task 1 exact-SHA compile repair:
- Run `44487763058` on `ac868a6` exposed a single compile error: rustls 0.23's `ServerName::IpAddress` uses `rustls::pki_types::IpAddr`, while the SAN parser uses `std::net::IpAddr`. Fixed the comparison with the explicit `Into` conversion; also moved the test-only `Path` import out of production scope to remove the warning.
- Commit `532f4462aca8245a9973fc6437a6cc6b9a3cd7ab` pushed. The old `phase1-rust` run was cancelled after the exact compile error was diagnosed; new CI will run on `532f446`.
- Fresh exact-SHA `phase1-rust` run `44488028597` is running on `532f4462aca8245a9973fc6437a6cc6b9a3cd7ab`. The same-SHA phase0/package workflows are also being tracked from the push; Task 1 is still unaccepted pending the full run.


Task 1 acceptance checkpoint (2026-11-19):
- Exact HEAD `532f4462aca8245a9973fc6437a6cc6b9a3cd7ab`.
- `phase1-rust` `44488028597`: SUCCESS, all six jobs successful.
- `phase0` `44488007805`: SUCCESS.
- Same-SHA package/signature `44488007895`: SUCCESS.
- Task 1 report: `.superpowers/sdd/2026-11-19-fleet-join-transport/task-1-report.md`.
- Scope remains transport primitives only; no listener, framing, Enroller integration or two-host LAB claim.


Task 2 acceptance checkpoint (2026-11-19):
- Exact HEAD `5699c38849a714d657f09c0e708568df963de602`.
- `phase1-rust` `44512226284`: SUCCESS, all six jobs successful.
- `phase0` `44512181372`: SUCCESS.
- Same-SHA package/signature `44512181308`: SUCCESS.
- Test totals: 430 passed / 0 failed / 5 ignored, plus a secondary test binary 6 passed. Clippy and release build passed.
- Task 2 report: `.superpowers/sdd/2026-11-19-fleet-join-transport/task-2-report.md`.
- Task 3 is next: bounded one-shot Hub listener and pinned member session; Enroller/Report integration and two-host LAB remain later gates.
