# Prioritized Implementation Proposal (from the engine ecosystem research)

- Status: proposal v1.0, 2026-11-08. Docs only. Nothing here is implemented or approved by this document.
- Inputs: `ENGINE_ECOSYSTEM_COMPARISON.md`, `REVERSE_ARCHITECTURE_COMPARISON.md`, `ENGINE_INTEGRATION_FEASIBILITY.md`, `ENGINE_LICENSE_AND_DEPENDENCY_MATRIX.md`.
- Unchanged by this proposal: Rust core as control plane, generic Go launcher, independent engine processes, versioned capability contract, existing `tcp-tls` and `noise-tcp` engines, ADR-0021 (Reverse stays unavailable), Ubuntu 22.04 / systemd 249, Direct stability, honest PASS / WARNING / BLOCKED / UNKNOWN states, no unsupported engine in the Wizard.

## 1. Priority list

Ordered by value divided by risk. "LAB" items need the shared LAB hosts, which other threads hold now; they are sequenced, not started.

| # | Item | Why | Risk | Needs LAB | Depends on |
|---|---|---|---|---|---|
| 1 | **Reverse ingress: bounded wait when no service session exists** (the small PR in section 2) | All references bound or refuse visitors without a session; ours queues them with no wait deadline (visitors are `try_send`-ed into the bounded queue and the consumer `run_forward_session` receives without a timeout, `src/tunnel.rs`; read, not run). Fixes a real hang class before the LAB gate. | Very low: touches only the Reverse path, which is not advertised | No (in-process tests) | none |
| 2 | **Extend the ADR-0021 LAB gate** with the added cases from `REVERSE_ARCHITECTURE_COMPARISON.md` section 5 (silent path cut, half-open stale session, no-session visitor, 500 concurrent visitors, public-node restart, inbound blocked all run) | Cheap way to catch the bugs the reference projects hit (chisel dead-path ping, rathole/frp stale control) | None (test plan) | Yes, later | item 1 |
| 3 | **Run the Reverse two-host acceptance gate**, `tcp-tls` first, `noise-tcp` separately; flip the capability only on PASS | Delivers the headline feature honestly | Medium (protocol-affecting, but gated) | Yes | items 1, 2; LAB availability |
| 4 | **Runtime Host readiness kind `unix_status`** (manifest schema bump, generic probe: connect, send status op, require a field) | Lets dialing-role engines (BackPack client, chisel client, any reverse service node) report Ready on `connected`, not on a local listener. Also fixes the ADR-0020 limitation that TCP accept does not prove a peer is connected. | Low-medium: schema change, unit-testable without LAB | No | none (can go before 3) |
| 5 | **ADR-0022: shared-secret engine class** (capability flags `peer_identity=shared_secret`, `revocation=rotate_only`, WARNING in wizard/preflight, per-tunnel derived secret) | Prerequisite for any token-authenticated engine (BackPack, chisel password mode, frp). Keeps ADR-0018 clause 1 intact for stronger engines. | Low (docs + capability field) | No | none |
| 6 | **ADR-0023: copyleft separate-binary engines (BackPack)** — accept AGPL-3.0 + NOTICE s.7 obligations, release source archive, rename policy for modified builds | Legal gate before any BackPack artefact enters the manifest | Low technically, legal commitment | No | owner acknowledgement (section 3) |
| 7 | **BackPack engine spike, hidden from the Wizard**: reproducible CI build of a tagged commit (two builds, equal SHA), `govulncheck` + licence report, pure Rust renderer with golden tests like `baft_engine`, hardened unit drop-in; LAB comparable matrix | Turns the best Go candidate from "plausible" into "measured". | Medium | Yes | items 4, 5, 6 |
| 8 | **Measured route test in preflight** (Phormal idea): non-destructive per-engine reachability and path probes between the two nodes via the agent API, with confidence level and evidence file; recommendation derived from measurements, not a static rank table | Gives the wizard real PASS/WARNING/BLOCKED/UNKNOWN per engine and path; the owner's top product idea from Phormal | Medium (agent-to-agent probing; must be consent-based and read-only) | Partly | existing preflight, agent API |
| 9 | **Transport fallback chain in the supervisor** (BackPack idea): ordered `engine_fallbacks` per ADR-0018 clause 2, same list on both ends, dwell-based meeting, capped backoff | Direct value against transport-specific filtering; ADR-0018 already commits to it | Medium | Yes | at least two working engines |
| 10 | **Native `ws-tls` Rust engine** (rathole/chisel as references) for camouflage over WebSocket | ADR-0018 engine #4 | Medium | Yes | items 3, 9 |
| 11 | Evaluate chisel as an MIT, SSH-pinned independent engine for Reverse over WebSocket | Cleanest licence among Go candidates | Medium | Yes | items 4, 5 |
| 12 | Operational ideas backlog: frp-style `allowPorts`, bandwidth limit, Prometheus metrics | Nice to have | Low | No | later |

Do not do: integrate spoof-tunnel, Pahlavi-tunnel, rathole-tunnel or TAQ-BOSTAN/Phormal scripts; copy GPL, AGPL or unlicensed code into the Rust tree; run upstream installers on any host.

## 2. The one small implementation PR (proposed, not started)

**Title:** Reverse ingress: close visitor connections that wait too long for a service session.

**Capability:** bounded wait and clean close for user connections accepted on the reachable node while no authenticated service session is active.

**Why this one:** highest value for the lowest risk. It is a correctness gap that every reference project handles explicitly and that the LAB gate would otherwise discover late; it affects only the Reverse code that is already reported unavailable, so Direct, `tcp-tls`, `noise-tcp` and the capability states cannot regress.

**Scope (files):** `src/tunnel.rs` only (ingress queue consumer around the `IngressSide` receiver), plus tests in the same module.

**Behaviour:**
- Record the enqueue time of each queued visitor connection.
- When a session is active, behaviour is unchanged.
- When a visitor has waited longer than a constant (proposed 30 s, named `INGRESS_QUEUE_MAX_WAIT`), close it instead of holding the socket; log one line with a count, no addresses beyond what existing logs print.
- Queue bound (`max_streams`) and the existing full-queue rejection stay.

**Verification (no LAB):**
- In-process test: no service session, visitor connects, connection is closed within the bound (time-paused tokio test).
- In-process test: a session arrives before the bound, visitor is served and bytes round-trip (reuse the existing reverse e2e helper at 256 KiB).
- Existing reverse and Direct tests, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` pass.
- No change to capability output, wizard, preflight or ADR-0021 text.

**Not in scope:** keepalive tuning, wire protocol, config schema, capability flip.

## 3. Decisions needed

| # | Decision | Recommendation | Note |
|---|---|---|---|
| D1 | Accept AGPL-3.0 + NOTICE s.7 obligations for shipping an unmodified, pinned BackPack as a separate binary | Yes, conditional on unmodified builds, source archive in each release, and the renaming rule for any patched build | Legal commitment; the owner delegated engineering decisions on 2026-11-07, but this one creates a licence duty, so it is listed for explicit acknowledgement |
| D2 | Add the "shared-secret engine" capability class with a WARNING state | Yes | Honest alternative to refusing all token-authenticated engines |
| D3 | Allow a schema bump of `engine-manifest.json` for a second readiness kind | Yes | Needed for any dialing-role engine |
| D4 | Provide the exact URL of M-R-R-X/ctunnel if it is meant to be a different repository | Optional | Could not be verified; excluded |

## 4. Next actions

1. Implement the small PR in section 2.
2. Open ADR-0022 and ADR-0023 (docs only) once D1 and D2 are acknowledged.
3. Hand the extended gate (item 2) to the thread that owns LAB access; no LAB job is started by this research.
4. After the Reverse gate result, decide the engine order for items 7, 10 and 11 with the comparable LAB matrix from `ENGINE_INTEGRATION_FEASIBILITY.md` section 6.
