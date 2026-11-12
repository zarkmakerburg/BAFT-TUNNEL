# BAFT-TUNNEL — Verified Capability Alignment & Agent Execution Specification

- **Document status:** verified repository assessment + binding execution guidance for the next implementation cycle.
- **Assessment date:** 2026-11-08.
- **Repository:** `zarkmakerburg/BAFT-TUNNEL`.
- **Code baseline:** `main` at `05a2b8078cc7afae751520209dba7fd474914bfb` (PR #86).
- **Current status correction (2026-11-12):** the assessment below was taken at `05a2b80`; current `main` is `32bf85f10d75efafc0d5f7a45a25d060f7f7c9b6`. B1 is merged (`311b706`, `62d7ee6`) and the B2 implementation-start prerequisites are satisfied ([ADR-0028](ADR-0028-authenticated-integrity-probe.md), the accepted Option A policy in [PROTOCOL_V2_COMPATIBILITY_MATRIX.md](PROTOCOL_V2_COMPATIBILITY_MATRIX.md) and the contract vectors in `tests/protocol_v2_contract.rs`); the B2 runtime probe is **not** implemented (the protocol is still v1) and is tracked in PR #177.
- **Concurrent implementation (historical):** PR [#88](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/88), head `969154c0b02dee5656ba91cf410a8be3da006061`; open at the assessment baseline and **merged since**. Its real-systemd gate (P0-1) closed on 2026-11-08 in hosted run [42523536944](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42523536944) on SHA `22e94f1`, and the two-host proof followed in P0-5 ([run 42538458094](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42538458094), main `92c4e70`). U3 (PR #137) later added stop/restart and boot persistence.
- **Authority:** this document supplements existing ADRs and acceptance policies. It does not silently amend an ADR, authorize a merge/release/deploy, or promote an experimental capability to stable.

## 1. Purpose and decision summary

This document replaces the earlier high-level capability extraction as the working technical reference. It checks proposed ideas from BAFT, Hashem Panel and BAFT BCC against the current BAFT-TUNNEL source tree, merged changes, design records and reported test evidence.

The product direction is:

> **Keep BAFT-TUNNEL an independent, security-first server-to-server connectivity product. Keep the Rust runtime and local operator workflow small and verifiable. Adopt BAFT's transport/runtime lessons, Hashem's infrastructure-operations concepts, and BCC's operator UX patterns only where they fit the current product boundary and can be backed by real APIs and tests.**

Do not turn BAFT-TUNNEL into a copy of any reference product. In particular:
- BAFT-TUNNEL is not a subscription/billing panel. The architecture explicitly positions its API for integrations with other panels.
- The current panel is a local operator UI served by the agent; it is not yet a central multi-node fleet console.
- A capability visible in source code is not automatically a supported product capability.
- The BAFT main engine is a separate pinned Go process. It must not be collapsed into the Rust `StreamEngine` abstraction or copied into the proprietary Rust tree.
- Reverse for `tcp-tls` is **Preview**, not Stable. Reverse for `noise-tcp` is unproven and must remain unavailable.
- PR #88's Apply `start` stage was not part of `main` at this baseline; it has since merged and is LAB-proven (P0-1 and P0-5), and U3 added stop/restart with boot persistence. Apply still never claims `ready`, which stays Unknown by design.

## 2. Evidence and verification rules

Use these labels in all implementation reports and HQ entries:

| Label | Meaning |
|---|---|
| **MAIN-CODE** | The capability is present in the inspected `main` source at the baseline SHA. |
| **MERGED-PR** | A merged PR describes the change; inspect current main before relying on it. |
| **LOCAL-TEST-CLAIM** | A PR author reports local tests; this is not an independently observed CI/LAB result. |
| **CI** | A completed GitHub Actions result tied to the exact SHA and workflow. |
| **REAL-LAB** | A completed two-real-host test with run ID, exact code SHA, result fields and cleanup evidence. |
| **PARTIAL** | Some layers or stages work; the full operator outcome is not proven. |
| **PREVIEW** | Implemented and boundedly tested, but not eligible for Stable or production claims. |
| **PLANNED / NOT VERIFIED** | Design or code exists without required execution evidence. |
| **BLOCKED / UNAVAILABLE** | Must not be offered as an operational choice. |

**Assessment limitation:** this is a source/document/PR evidence review, not a new build, formal security audit, or fresh LAB execution. PR descriptions are not treated as proof when they explicitly say a test has not run. The GitHub status lookup for PR #88 did not return a combined check result in this assessment; do not infer success or failure from an empty status response. (Historical: PR #88 later merged and its real-systemd gate closed in run 42523536944.)

## 3. Verified state of the current product

### 3.1 Runtime and engines

| Engine / path | State in current main | What may be claimed | What must not be claimed |
|---|---|---|---|
| `tcp-tls` Direct | Supported; TLS 1.3 mutual authentication, certificate identity, SHA-256 pinning, CRL checks, bounded Yamux streams | Real two-host Direct evidence and prior 3600-second / real 2% TCP-loss evidence recorded in the implementation history | Overall product production-ready |
| `tcp-tls` Reverse | Real reverse data path; offered by capability contract and Wizard as **Preview** after run 42506805986 | Outbound-only service node; 64 KiB + 2 MiB integrity; both-node reconnect; busy-port refusal; wrong identity, wrong pin and revoked certificate refusal | Stable, production-ready, soak/loss/agent-restart accepted |
| `noise-tcp` Direct | Experimental; Direct two-host evidence exists | Experimental authenticated static-key stream path | Equivalent maturity to `tcp-tls` or hidden fingerprint/undetectability |
| `noise-tcp` Reverse | Not offered; current capability catalog lists Direct only | Local implementation work may exist; current source does not advertise it | A successful Reverse data path. Historical `reverse_ok` evidence was label-only and is explicitly invalidated by ADR-0021 |
| `baft` | A separate-process engine adapter exists in code; configuration routes `EngineKind::Baft` to `baft_engine::run`; Store resolution can select a hash-verified current release | A prior two-host LAB demonstrated the BAFT engine host under hardened systemd 249 | That the *new Engine Store → Launcher path* has passed real-host LAB, or that BAFT is ready to be exposed in the Wizard |
| `ws-tls`, uTLS sidecar, QUIC/UDP, auto-rotation | Research / future scope; not in the current advertised capability catalog | Candidate work after need, design and acceptance are approved | Implemented or selectable transports |

Primary source evidence:
- [ADR-0018 / engine framework](ADR-0018-engine-framework.md)
- [ADR-0019 / BAFT engine host](ADR-0019-baft-engine-host.md)
- [ADR-0020 / managed runtime host](ADR-0020-managed-runtime-host.md)
- [ADR-0021 / topology and Reverse](ADR-0021-connection-topology-and-reverse.md)
- [Engine capability contract](../src/capabilities.rs)
- [StreamEngine interface](../src/engine.rs)
- [BAFT adapter](../src/baft_engine.rs)
- [Engine Store](../src/engine_store.rs)

**Important Engine distinction:** `StreamEngine` creates an authenticated secure byte stream. The shared Rust tunnel layer continues to own the hello/authorization contract, Yamux, stream limits and connection handling. BAFT owns its own Session/Flow multiplexing and data plane; it is integrated as a separate supervised process, not as a normal `StreamEngine`. A generic managed-process lifecycle is a separate concern from a stream-engine trait.

### 3.2 BAFT Engine Store and launcher

Merged PR #85 connects `baft.store` to the launcher:
1. If `baft.store` is set, the launcher resolves the Store's `current` binary.
2. The binary is re-verified against the manifest stored beside it.
3. The resolved SHA-256 must match the SHA pinned in the tunnel config; a mismatch fails closed.
4. Without `baft.store`, the pinned `baft.binary` path is verified and used.

**Update 2026-11-08 (REAL-LAB):** the Store path has real-host evidence. [Run 42510577167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42510577167) on `2d2718b` (local-build engine) proved store install/verify, hardened systemd 249 launch through `baft.store`, engine-kill recovery and wrong-pin refusal. [Run 42522105698](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42522105698) on `06c273e` repeated it with the **signed `v0.1.0-rc.2` release asset** (cosign-verified `SHA256SUMS`, digest equal to the manifest pin) and added rollback: release B installed over A, a pin that is not `current` refused, B carried 2 MiB, rollback to A, pin B then refused, rollback to a tampered release refused, A carried 2 MiB again, cleanup OK. Keep BAFT absent from `/v1/engines` and the Wizard until the owner-approved integration gate is closed; that exposure gate is separate from this LAB gate.

Also correct an obsolete comment at the top of `src/baft_engine.rs` that still says the adapter is not wired into engine selection. The executable path is wired through `RuntimeConfig::apply`; the capability catalog intentionally still hides BAFT. Update the comment only in a focused code PR with tests, not as part of this documentation-only change.

### 3.3 Agent, API and Apply lifecycle

The current agent exposes a **local** authenticated API, including:
- `GET /v1/status`, `/v1/history`, `/v1/security`, `/v1/audit`, `/v1/engines`, `/v1/desired`
- `PUT /v1/desired`
- `POST /v1/setup/plan`, `/v1/setup/preflight`, `/v1/setup/apply`

The current Apply implementation on `main` (ADR-0022 / PR #86) supports only local file/setup stages:
- `init`: local key or PKI/pairing preparation and public bundle.
- `join`: validate peer bundle and write/validate local configuration.
- server-side preflight, explicit confirmation, idempotency key, receipt, and rollback of files created by that operation.

It does **not** start/enable a service, change/inspect firewall state, contact/authenticate the peer, or verify end-to-end application data. The UI's Apply wording must continue to say exactly what is done and not done.

PR #88 added `start` through narrowly-scoped systemd path units and was **open at the assessment baseline with local fake-controller evidence only**. Every follow-up condition it needed has since been met:
- The real Ubuntu 22.04/systemd 249 lifecycle gate (P0-1) closed on 2026-11-08 in hosted run [42523536944](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42523536944) on SHA `22e94f1`: start/stop, scope restriction, refusal of invalid or unregistered instances, failure rollback, idempotent/stale requests, no unrelated state change and cleanup.
- The two-host proof in P0-5 ([run 42538458094](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42538458094), main `92c4e70`, signed `v0.1.0-rc.4`) recorded `apply_start_serve/forward=applied` and `receipt_session=observed`.
- U3 (PR #137, LAB [42692143748](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42692143748)) added Apply `stop`/`restart`, boot persistence and the agent unit.
- Apply still never claims data integrity or `ready`; Verify/LAB measures those and `ready` stays Unknown by design.
- PR #88's history is preserved: do not rewrite it or re-implement the same change elsewhere.

### 3.4 Preflight and truthful status

Current Preflight:
- Reuses the canonical setup planner.
- Enforces engine/direction capability.
- Runs local directory/config checks.
- For a Forward role with a literal IP, makes a bounded outbound TCP connect to the peer's tunnel port. This proves only that a TCP listener answered; it does **not** authenticate the peer.
- Reports peer identity and firewall inspection as **Unknown**.
- The listener-side role cannot test peer reachability from its side and leaves it Unknown.

Current `GET /v1/status` checks whether the configured local listener accepts a TCP connection. It does not prove an authenticated peer session or successful forwarded data. UI copy already warns that the peer connection is not verified.

**Readiness contract:** retain separate states for host reachability, TCP reachability, authenticated peer, authorized session, listener readiness and bidirectional data-path integrity. Only the final readiness label may say Ready when all required fresh checks pass. Otherwise show Degraded, Needs attention, Offline or Unknown, with evidence source and freshness. Do not collapse these into one green LED.

### 3.5 Panel and UX reality

The current panel is a dependency-free static HTML/CSS/JavaScript UI under `panel/`, served by the local agent. It is not currently a React/TypeScript/Vite application. The architecture and some README/roadmap text still mention React/Vite; source code and the panel implementation record show the actual MVP panel has no framework or build step. Keep the current low-dependency implementation unless a reviewed product decision justifies a frontend migration.

Current pages / journeys include:
- Overview
- Servers (local node / configured peer view, not an enrolled fleet)
- Tunnels (configured tunnel view)
- Create Tunnel Wizard (six-step flow using the canonical setup planner)
- Agent
- Activity
- Settings

Current UX strengths worth preserving:
- Persian RTL and English LTR, including direction-aware navigation and isolated LTR technical identifiers.
- Dark default and theme option.
- Responsive desktop navigation and mobile tab navigation.
- Empty, unknown, offline and stale states rather than stale-green status.
- Preflight, setup preview and local Apply `init/join` with receipt.
- Semantic DOM text creation; no raw HTML injection path in the page helper.

Current limitations:
- “Servers” is not a Fleet: there is no enrolled-server inventory or remote state.
- Tunnel view does not verify the authenticated peer/data path and does not provide full remote lifecycle actions.
- Traffic counters / throughput are not exposed by the API; do not invent chart values.
- No firewall inspection or firewall mutation.
- Audit v2 records actor, target and resource semantics (PR #109); structured evidence localization is delivered in B1 and merged into `main`.
- Structured Preflight/Diagnostics evidence codes + typed params and fa/en localization are implemented in B1 and **merged into `main`** (`311b706`; the duplicate `diag.next.*` keys that shadowed explanatory translations were removed with a regression assertion in `62d7ee6`). Original implementation CI passed at `cc37f48`; U8 is merged in PR #165. The synchronized head `ec730c3` in PR #168 passed all four exact-head workflow groups (Rust, Ubuntu 22.04/systemd 249, panel-ui, offline CA, phase0, fuzz-smoke and same-SHA package/signature); that run remains historical evidence, not a current-head claim.
- Physical-phone UX and formal accessibility audits are not recorded as complete.

A separate BCC-style Connection Test is **not yet equivalent to an authenticated end-to-end diagnostic**. The current Preflight TCP probe must not be relabelled as a successful tunnel/data-path test. “Add from Link” is also not verified as an existing BAFT-TUNNEL workflow; treat it as a later feature request requiring an explicit safe link schema and validation.

## 4. How the reference projects should influence the product

### 4.1 BAFT — adopt engineering lessons, not architecture by imitation

Adopt:
- Explicit session and flow lifecycle thinking.
- Flow-control, bounded queues, resource accounting and backpressure.
- Recovery/reconnect semantics with measurable recovery limits.
- Per-peer identity and authorization, separate from mere connectivity.
- Protocol/transport capability declarations backed by conformance tests.
- Runtime metrics and privacy-aware telemetry, without payload logging.

Do not:
- Re-implement BAFT's Session/Flow protocol inside the Rust transport interface merely to make the interfaces look alike.
- Copy BAFT BCC, agent, cluster, installer or operational code into BAFT-TUNNEL.
- Claim process-restart continuation of active flows without a protocol and test proving it.
- Merge transport features into the core without a concrete use case and acceptance matrix.

BAFT's value is its independent Go Runtime and data-plane design. Current integration's process boundary and hash-pinned release model must remain explicit. BAFT's licence and attribution obligations remain attached to the BAFT artifact; the boundary is an engineering/licensing control, not an automatic legal exemption. Keep the separate binary, source/licence manifest and distribution obligations under review. Obtain legal review if distribution terms or packaging change.

### 4.2 Hashem Panel — adopt operations concepts in the right phase

Candidate concepts:
- A central view of Nodes and their freshness/health.
- Multi-node tunnel orchestration and change history.
- Version/update visibility, backup/restore planning and operational diagnostics.
- Explicit action previews, least privilege, audit and recoverable operations.

Current fit:
- **Later phase**, after the two-host product lifecycle is complete.
- Start with a small authenticated Node inventory and read-only remote status; only then consider fleet-wide mutation.
- Every remote action needs node identity, authorization scope, an idempotent Job, result receipt, timeout and recovery behavior.

Do not import Hashem code or silently enlarge the current MVP into a customer-management suite. The present BAFT-TUNNEL architecture says it is not a subscription panel. Billing, reseller tiers, volume plans and customer accounts should remain an external integration concern unless the owner explicitly changes the product boundary.

### 4.3 BAFT BCC — adopt operator UX, not unsupported actions

Keep / strengthen:
- Overview with an honest system verdict and one useful next action.
- Tunnel inventory and search/filter once the backend can provide stable tunnel records.
- A guided, progressive-disclosure Wizard.
- A real diagnostic journey that distinguishes TCP reachability, handshake/identity, authorization, session and data integrity.
- Simple setup summaries and copyable diagnostics.
- Responsive mobile layouts and Persian RTL/English LTR.

Do not expose buttons for “Deploy best”, Direct/Reverse selection, firewall changes, update/rollback or destructive actions unless the backend has the authority, preview, verification and receipt contract for that operation. Capability-driven UI must be generated from `/v1/engines` and current Preflight, not from hardcoded assumptions.

## 5. Capability matrix — current state versus target

| Capability | Current state | Target / decision |
|---|---|---|
| TCP-TLS Direct | Supported with real-host evidence | Preserve; continue regression coverage |
| TCP-TLS Reverse | Preview; dedicated two-host LAB pass plus R3 breadth REAL-LAB pass (P0-4, run 42525790470) | Owner review before any Stable promotion |
| Noise-TCP Direct | Experimental, Direct LAB pass | Keep Experimental; test impairments before promotion |
| Noise-TCP Reverse | Unavailable / unproven | Keep hidden until its own real data-path gate |
| BAFT engine | Separate process, config adapter and supervised launcher; Store path, rollback and signed asset REAL-LAB passed (P0-3) | Keep Wizard-hidden until an owner-approved exposure gate |
| Local setup CLI | Implemented; setup plan/init/join and prior two-host setup evidence | Make the signed-release path the actual operator acceptance path |
| Local Agent/API | Implemented | Extend only with evidence-backed lifecycle and session telemetry |
| Apply init/join | Implemented on main | Preserve receipts, idempotency and rollback boundary |
| Apply start/stop | Implemented and LAB-proven (P0-1/P0-5; stop/restart and boot persistence via U3, PR #137) | Keep receipts truthful; `ready` stays Unknown |
| Local preflight | Implemented; peer TCP probe is not peer identity | Add authenticated peer/session/data-path diagnostics |
| Panel | Implemented local static UI | Keep simple; align docs with source; no React migration by default |
| Fleet / remote Nodes | Not implemented | Later Control Plane phase |
| Throughput / full traffic metrics | Not exposed by current API | Add measured counters with units, interval and freshness |
| Firewall inspection/management | Not implemented | Read-only inspection first; mutation requires separate scope and rollback design |
| Customer/subscription/reseller billing | Outside current product boundary | Do not add to core MVP; integrate via external API later |
| Backup/restore orchestration | Not proven in current implementation | Later, with tested restore—not merely backup creation |
| WebSocket, uTLS, QUIC/UDP, automatic rotation | Future research / absent from capability contract | Add only after use case, ADR, security review and two-host test matrix |

## 6. Revised priorities

### P0 — close the real product lifecycle

**P0-1 — PR #88 systemd control helper — CLOSED 2026-11-08**
- Review unit templates, helper script, file ownership/modes, instance-name validation, symlink/race behavior, stale request cleanup, replay, and scope restriction.
- Confirm the agent cannot run arbitrary commands or directly control unrelated systemd units.
- Real Ubuntu 22.04/systemd 249 LAB: install helper for one disposable instance; start; verify stable service; stop; refuse unregistered instance; refuse malformed instance; failure during start must stop/rollback; exercise duplicate/stale requests; prove no unrelated service/port/firewall is changed; cleanup all temporary resources.
- Preserve exact logs without secrets and tie the result to code SHA + run ID.
- **Result:** the gate passed on 2026-11-08 in hosted run [42523536944](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42523536944) on SHA `22e94f1`; PR #88 is merged and the two-host lifecycle proof followed in P0-5. A green unit test was not accepted as a substitute for this LAB.

**P0-2 — Apply/Verify/Receipt truth and real data-path readiness**
- Keep `init` and `join` semantics unchanged.
- Do not call a started listener a connected tunnel.
- Define/implement observable authenticated-session and bidirectional data-path probes.
- Record each stage independently in receipt: files applied, service state, authenticated peer, authorized session, data-path integrity, rollback and what remains Unknown.
- Any missing observation stays Unknown; never return an overall Ready result from listener-only evidence.

**P0-3 — BAFT Engine Store real-host verification** — *LAB gate closed 2026-11-08: runs 42510577167 and 42522105698 (signed rc.2 asset), see §3.2.*
- Use the pinned signed engine artifact and its matching `release/engine-manifest.json`.
- Install into Store, verify `current`, run BAFT through `baft.store` under the hardened systemd 249 service, verify data, kill child and prove supervisor recovery.
- Change the configured pin to a different SHA and prove launch is refused.
- Prove rollback requires/uses the matching config pin and never executes a binary whose digest differs.
- Record whether this path was run from a signed release asset or a locally built binary; these are not interchangeable evidence.
- Keep BAFT out of the capability catalog/Wizard until an explicit owner-approved release gate says otherwise.

**P0-4 — TCP-TLS Reverse R3** — *LAB gate closed 2026-11-08: run 42525790470 on `5a72a25`; UI stays Preview pending review; Noise Reverse not tested and stays unavailable.*
- Extend beyond the existing single pass: agent restart, repeated reconnect/replace, stale-session handling, port conflict, interruption at different lifecycle stages, longer soak, packet loss and cleanup.
- Keep current UI state Preview until this evidence is reviewed.
- Test Noise Reverse separately; do not inherit TCP-TLS results.

**P0-5 — Integrated two-host MVP acceptance** — *LAB gate closed 2026-11-09: run 42538458094 on `92c4e70` with signed prerelease v0.1.0-rc.4; see PHASE1_IMPLEMENTATION_STATUS. Setup init/join used the guided CLI with bundle checksums; Apply drove start. Not production-ready.*
- Start from clean Ubuntu 22.04/systemd 249 test hosts.
- Install the signed pre-release artifact, not only a locally compiled executable.
- Complete setup using the supported CLI/Wizard path with bundle integrity checks.
- Exercise Apply init/join and the accepted start path; confirm mTLS identity/authorization and 64 KiB + 2 MiB bidirectional integrity.
- Test interruption/recovery, wrong identity, wrong pin, revoked certificate, occupied port, rollback and uninstall.
- Record exact commit, artifact digest, run ID and cleanup proof. This is the gate before claiming an operator-ready MVP or cutting a new pre-release.

### P1 — evidence-backed operations and diagnostics

1. Authenticated Node enrollment and a minimal remote Node inventory (only after an ADR and control-plane trust boundary).
2. Peer-aware diagnostics: separate TCP reachability, authenticated handshake, identity, authorization, session and data-path probe.
3. End-to-end tunnel status and last-verified time.
4. Audit actor/target/resource fields and durable remote Job receipts.
5. Real traffic/throughput counters (bytes, rate, sampling interval, units and freshness).
6. Structured Preflight/Diagnostics evidence codes + typed params and Persian/English localization (**merged** into `main`: `311b706`, duplicate-key fix `62d7ee6`; historical exact-head CI in PR #168).
7. Read-only Firewall inspection, with explicit Unknown where unavailable.
8. “Add from Link” only after a versioned import schema, checksum, strict validation, secret redaction and preview are designed.

### P2 — multi-node product operations

1. Fleet inventory, Node groups, versions and health freshness.
2. Remote orchestration by bounded Jobs with authorization, idempotency, receipts and rollback.
3. Safe update/rollback and verified backup/restore workflows.
4. Notifications and operational history.
5. External panel/customer/subscription integration APIs, if the owner prioritizes that business direction.

### P3 — new transports and advanced automation

Evaluate WebSocket/TLS, uTLS sidecar, QUIC/UDP, relay and automatic engine/path selection only after:
- a real user problem is documented;
- the engine/transport contract and security model are reviewed;
- licence/dependency and provenance checks pass;
- failure, resource, downgrade and compatibility tests are defined;
- real-host acceptance exists for the specific path.

## 7. Non-negotiable product and engineering rules

1. **No false readiness:** host reachable ≠ TCP reachable ≠ authenticated peer ≠ authorized session ≠ data path verified.
2. **No fake capability:** mock, local, CI and real-host LAB evidence are named separately.
3. **Preview stays Preview:** no stable claim from a single bounded pass.
4. **Capability source of truth:** UI, CLI, Preflight and API must agree with `/v1/engines`; unavailable modes cannot be selected or applied.
5. **Default-deny authority:** all mutations are scoped, explicitly confirmed, auditable, bounded and recoverable.
6. **Least privilege:** the agent cannot become a generic root command runner. Any privileged helper must have a tiny fixed command surface and a real-host test.
7. **No payload logging:** diagnostics must not expose tunnel payloads, private keys, tokens or CA signing material.
8. **No unrelated host changes:** do not alter existing firewall, routes, sysctl, service or files without a separate reviewed operation.
9. **No architecture drift:** Rust remains the core. External engines stay behind reviewed process/adapter boundaries.
10. **No silent licence change:** BAFT and third-party engine licences, notices, manifests and source obligations must remain attached to their artifacts. This document is not legal advice.
11. **Keep the panel simple:** default workflows hide engine internals; experts can open advanced diagnostics without making the default wizard technical.
12. **One LAB run at a time:** shared Iran/abroad LAB hosts must not be used concurrently by independent workers.

## 8. Required agent workflow

The implementation agent receiving this document must follow this order:

1. **Re-read live state first.** Fetch latest `main`, Issue #2, open PRs, workflow checks and exact code SHAs. This document's main SHA is a snapshot, not permission to ignore later merges.
2. **Protect in-flight work (historical).** PR #88 has since merged; do not rewrite its history, reset it or force-push, and do not re-implement the same change elsewhere.
3. **Reconcile status.** Update this document/Roadmap only when code, test and evidence state changed. Preserve historical failed runs and explain superseded evidence.
4. **Complete P0 in dependency order.** First close the Start helper review and real-systemd LAB; then data-path readiness; then BAFT Store path LAB; then Reverse R3; then integrated signed-artifact acceptance. *(Historical: all five P0 gates have since closed — see the [roadmap](PRODUCT_ROADMAP.md) for the exact SHAs and run IDs.)* Parallelize only tasks that do not share LAB hosts or source files.
5. **Implement only the smallest required slice.** Name exact files, API contract, side effects, authorization boundary, rollback scope and test plan before editing.
6. **Test at the effect boundary.** Unit/fake tests are necessary but insufficient for systemd, filesystem ownership, remote node, network path, engine process and cleanup behavior.
7. **Do not expand product scope.** Do not start billing/subscriptions, Fleet, a React migration, new transports or automatic rotation while P0 lifecycle gates remain open unless the owner gives a new explicit priority.
8. **Use focused PRs.** One coherent slice per PR. Do not merge or release merely because code is present; close the acceptance evidence first.
9. **Update evidence.** At each accepted milestone record branch, SHA, PR, exact test commands, CI URLs, LAB run ID, measured results, cleanup and remaining Unknowns in [HQ Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2), [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md) and the relevant status/ADR.
10. **Report honestly.** End each handoff with Done / Partial / Blocked, exact evidence and the next one action. Never mark a test PASS if it was not executed.

## 9. Acceptance gates

A task is complete only when all applicable rows are satisfied:

| Gate | Required evidence |
|---|---|
| Source | Merged/current main SHA and bounded diff; no accidental unrelated edits |
| CI | Required exact-SHA workflow checks completed successfully |
| Local tests | Commands and counts recorded; no claim beyond what was run |
| Real host | Required two-host/systemd/network path actually executed |
| Security | Identity, authorization, least privilege, negative cases and secret handling tested |
| Lifecycle | Apply, verify, receipt, idempotency, failure rollback and cleanup tested |
| UX | User-visible state matches backend evidence; unavailable capability stays disabled |
| Docs/HQ | ADR, roadmap, panel traceability and HQ issue updated with evidence |
| Release | Separate release gate; source SHA, signed artifact, manifest, SBOM/licence material and checksums verified |

## 10. Source-of-truth references

- [Product Roadmap](PRODUCT_ROADMAP.md)
- [HQ Control Board / Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2)
- [Architecture](ARCHITECTURE.md)
- [Engineering guardrails](ENGINEERING_GUARDRAILS.md)
- [BAFT lessons and guardrails](BAFT_LESSONS_AND_ENGINEERING_GUARDRAILS.md)
- [Product UX acceptance](PRODUCT_UX_ACCEPTANCE.md)
- [Panel-to-HQ traceability](PANEL_HQ_TRACEABILITY.md)
- [Phase 1 implementation evidence](PHASE1_IMPLEMENTATION_STATUS.md)
- [ADR-0021: connection topology and Reverse](ADR-0021-connection-topology-and-reverse.md)
- [ADR-0022: Apply, Verify, Receipt](ADR-0022-apply-verify-receipt.md)
- [PR #88 — agent control helper](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/88) — merged; its real-systemd gate closed 2026-11-08 ([run 42523536944](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42523536944))
- [BAFT engine integration design](BAFT_ENGINE_INTEGRATION_DESIGN.md)
