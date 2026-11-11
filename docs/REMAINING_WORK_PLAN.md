# Remaining work: assessment and prioritized plan

> **Verified status correction — 2026-11-11 (supersedes the 2026-11-09 assessment below):** the tables and findings that follow are retained as the historical assessment baseline, not current-state claims. Current main is `1219f547e6d9c2fe9605385f37a7d5dd8dab2995` (PR #162; U7 harness-only checksum fix). Trusted U7 run [43063199428](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43063199428): release-tag and asset verification passed; the two-host job remains in progress, so U7 is NOT VERIFIED yet. Q1, Q3 and Q4 are implemented; Q2 documentation reconciliation remains active. U1 is complete: [LAB run 42656507521](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42656507521) on ancestor SHA `8be74376176442106669f4a81293f0865629ecaf` renewed the CRL to 30 days, pushed to both nodes, advanced CRL number `0x1002→0x1003`, recovered the 2 MiB tunnel in 2 seconds, and refused short/replayed CRLs. U2/U3/U5a/U4/U5b are implemented; D2 was merged in [PR #150](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/150), and the trusted repeat [LAB run 42996224181](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42996224181) passed the two-host D2 lifecycle on the immutable RC.7 candidate. U6 is complete with signed prerelease `v0.1.0-rc.7`. The next integrated gate is U7 on the signed RC.7 package, capped at 60 minutes. The prior unrelated nftables-set delta was not reproduced in the D2 repeat (`host_drift=none`), but its source remains unattributed and stays separate under S10. No stable release or production readiness is implied. The owner has paused the independent pre-stable review; do not restart it without a new instruction. No soak longer than one hour is required or planned.


- **Date:** 2026-11-09. **Baseline:** `main` at `0a7576b` (PR #132), rechecked at `c7dbd15` after PR #133 merged (BAFT listed as Preview, Direct only, in `/v1/engines` and the Wizard; Reverse role wording). No finding below changed with #133.
- **Method:** read-only review of the source, docs, ADRs, HQ Issue #2 and the recorded CI/LAB runs. Nothing was built or run for this document; no LAB run, tag, release or deploy.
- **Evidence labels:** **[CODE]** read in the source at the baseline; **[DOC]** stated in a repository document; **[CI]** / **[LAB]** exact-SHA run evidence recorded elsewhere; **[INF]** inferred from the code, not executed.
- **Scope rule (owner):** finish P1 acceptance and panel integration; do not add engines or protocols.
- **Relation to the Roadmap:** [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md) §5 holds the delivery order and links here; this file holds the detail and the item-by-item status.

## 1. Verdict

**The engine room is proven; the operator journey is not.**

- **Proven [LAB]:** all five P0 gates; `tcp-tls` Direct two-host operations acceptance (run 42635190485); `tcp-tls` Reverse accepted as Preview (run 42619848756); BAFT accepted as Preview, Direct only (run 42552860541); P1 diagnostics, end-to-end status, traffic counters and audit (run 42545941726). The P0+P1 security assessment found no Critical or High issue ([SECURITY_ASSESSMENT.md](SECURITY_ASSESSMENT.md)).
- **Not yet usable by a non-expert operator** (the v0.2.0 promise in Roadmap §2):
  1. An installed `tcp-tls` tunnel needs about a dozen manual root steps per host plus the offline PKI ceremony (F2, F3).
  2. It stops reconnecting 7 days after its CRL was issued, and the PKI tool has no command to renew a CRL (F1).
  3. Neither the tunnel nor the agent comes back after a reboot (F3, F4).
  4. The panel cannot finish setup, and it cannot start, stop or restart a tunnel (F5).
  5. The signed release ships only binaries; the scripts and units operators run are outside the signature (F6).
- **Not production-ready.** No stable release. Receipts never say `ready` (by design, [READINESS_CONTRACT.md](READINESS_CONTRACT.md)).

## 2. Findings from this review

| ID | Finding | Evidence | Impact |
|---|---|---|---|
| F1 | CRL expiry with no renew path | [CODE] `scripts/pki/baft-pki.sh:89` `default_crl_days = 7`; subcommands are only `init-ca`, `new-node`, `issue-node`, `revoke-node`, `verify-node` (lines 217–221). [DOC] `OPERATOR_GUIDE.md:116` asks for weekly re-issue with at least 14 days validity, which the tool cannot produce. Expired CRLs fail closed on both TLS roles (tested). Panel warns only below 3 days (`panel/pages.js:51`). `push-crl.sh` is mock-SSH tested only. `apply-crl.sh:179` restarts the unit, so each CRL activation drops connections | Every `tcp-tls` tunnel built with the shipped tool stops reconnecting a week after setup unless the operator hand-writes `openssl ca -gencrl` |
| F2 | Manual root steps after setup | [CODE] each acceptance stage runs `chown baft-tunnel:baft-tunnel node.key && chgrp baft-tunnel NAME.json && install-agent-control.sh --instance NAME` as root (`lab/real-hosts/mvp-stage.sh:90`, `ops-stage.sh:91`, `reverse-accept-stage.sh:93`); [DOC] `OPERATOR_GUIDE.md:179` lists the same steps | Not "non-expert"; easy to get wrong |
| F3 | No agent service | [CODE] `systemd/` ships only `baft-tunnel@.service` and the control-helper units; [DOC] the agent is started by hand with `setpriv` (`OPERATOR_GUIDE.md:152–156`). SA-09 (agent and tunnel share one uid) is open | Panel and Apply disappear after a reboot or a crash of the shell that started the agent |
| F4 | No stop/restart, no boot persistence | [CODE] Apply stages are `init`, `join`, `start` only (`src/apply.rs:23–31`); stop is used only for rollback. [DOC] ADR-0023: "Not enabled at boot" | A tunnel started through Apply is down after a reboot |
| F5 | Panel cannot finish setup on an installed host | [CODE] the Wizard calls Apply stage `init` only (`panel/pages.js:382`) with default dir `/etc/baft-tunnel` (`panel/pages.js:232`); `install.sh` makes that directory `root:baft-tunnel 0750` (`scripts/install.sh:182,191`) and the agent runs as `baft-tunnel`. Preflight checks only that the directory exists (`src/agent.rs:592`). [INF] creating files there fails with permission denied; no LAB stage runs Apply `init`/`join` through the agent (stages use the CLI as root). Join and start are not wired in the panel; the Tunnels page offers diagnostics but no lifecycle action | An Apply button that cannot work on the supported layout; setup still needs the CLI |
| F6 | Operator scripts are not in the signed release | [CODE] `.github/workflows/release.yml:87–107` stages the binary, the BAFT engine, licences, the engine manifest and the SBOM; install/uninstall/update/backup/restore/control-helper scripts and the systemd units are not released | Operators run unsigned scripts from a git checkout; signature covers only part of what they install |
| F7 | Exact-SHA CI gap on `main` | [CODE] `phase1-rust.yml` runs on PRs and on push to the deleted `phase1/rust-tcp-transport` branch, not on push to `main`; no workflow has a `schedule` | After a squash merge, `rust` and `ubuntu-2204-baseline` never run on the `main` SHA unless dispatched by hand; new RustSec advisories are only seen on the next PR |
| F8 | BAFT shown but not configurable | [CODE] since PR #133 (`c7dbd15`) `/v1/engines` and the Wizard offer `baft`, then `src/setup.rs:77–81` refuses it in plan and preflight and the review step shows that error (`panel/pages.js:285,369`) | Selectable-then-blocked conflicts with "unavailable modes cannot be selected or applied" (alignment §7 rule 4) until ADR-0019 phase F |
| F9 | Stale statements in docs | [DOC] `README.md:73` (review "waived ... not performed"), `:84` (React/Vite), `:87` (Noise "research-only"), `:100` (soak and real packet loss "untested"); `ARCHITECTURE.md:18–20` (React/Vite, Axum, SQLite; none is in `Cargo.toml`); `quickstart.en.md:49` and `quickstart.fa.md:47` ("no installer"), both still build from source; `PANEL_HQ_TRACEABILITY.md:29,32,33` list G2/G5/G6 open (closed by #109/#120); Roadmap header (baseline `92b0645`, "P1/P2 held", "only ... phase1-pre" released); HQ Issue #2 body §1 dated 2026-11-08 (Reverse "still reported unavailable"); `src/baft_engine.rs:6` "Not wired" although `src/config.rs:529` routes to it | Breaks the project's own "no fake capability" rule in both directions |
| F10 | Integrity only measurable in LAB | [DOC] READINESS_CONTRACT level 4 is reached only by LAB stages; the agent reports `integrity: unknown` always | Honest, but the product can never show level 4/5 or a "Test tunnel" result without an in-product probe |

## 3. Item-by-item status

### 3.1 Roadmap §4 "Not implemented or not yet accepted" (written at `92b0645`)

| Item | Now | Evidence / next |
|---|---|---|
| Apply `start` not enabled at boot; receipt `ready` unknown | **Done (boot)**: Start enables at boot, Stop disables, real reboot of both LAB hosts brought agent and tunnel back by themselves; `ready` unknown by design | PR #137, #140–#142; LAB 42692143748 (main `ff868a7`) |
| No end-to-end peer/session/data-path result in the status API | **Done** | ADR-0024, PR #109, CI 42545957126, LAB 42545941726 |
| No central enrollment / enrolled inventory | Open, post-MVP | L1 |
| Servers page is not a Fleet | Open, post-MVP | L2 |
| No live throughput counters | **Done** (in-memory history 1 h) | PR #109 `/v1/traffic`, panel PR #120; persistence B5 |
| No read-only firewall inspection | Open | B3 |
| Audit actor/target; machine-readable evidence codes | Audit **done**; evidence codes **partial** | PR #109; B1 |
| No billing in core | By design | — |
| No production release | Open | Prereleases only: `v0.1.0-phase1-pre`, `rc.2`, `rc.3` (superseded), `rc.4` |

### 3.2 P1 list (Roadmap §5)

| # | Item | Status | Next |
|---|---|---|---|
| 1 | Authenticated enrollment + small remote inventory | Not started; needs a control-plane ADR | L1 (post-MVP) |
| 2 | Peer-aware diagnostics | **Done** (ADR-0024) | BAFT has no diagnostics (unknown by design); B4 |
| 3 | End-to-end tunnel state and last verified time | **Done** | — |
| 4 | Audit actor/target/resource | **Done**; remote Job receipts wait for L1 | L1 |
| 5 | Traffic counters with units, interval, freshness | **Done**; history only in memory (1 h) | B5 |
| 6 | Machine-readable Preflight evidence codes | **Partial**: check ids act as codes and remediation is localized by id, but evidence text is English (gap G8) | B1 |
| 7 | Read-only firewall visibility | Not started | B3 |
| 8 | Safe "Add from Link" | Code done (U5b): `baft-tunnel://pair?v=1&b=` link, paste or upload, checksum compare and preview | LAB run 42779915532 PASS |

### 3.3 P2 list

| # | Item | Status | Next |
|---|---|---|---|
| 1 | Fleet inventory, groups, version and health freshness | Not started | L2 |
| 2 | Remote orchestration via scoped Jobs | Not started | L2 |
| 3 | Update/rollback and backup/restore with restore acceptance | **Done locally** (ADR-0025, PR #111; LAB 42635190485 Direct, 42619848756 Reverse): same-version only; panel shows read-only guidance | Cross-version S2; remote Jobs L2 |
| 4 | Monitoring, alerts, operational history | Not started | B5, L4 |
| 5 | Third-party integration API | Not started; owner priority | L4 |

### 3.4 Phase 6 (hardening and stable)

| Item | Status | Next |
|---|---|---|
| Longer reliability | 3600 s Direct (42412781202), 1800 s Reverse, 1200 s BAFT; nothing multi-hour | S1 |
| Backup/restore, update lifecycle | Done for the same version | S2 cross-version |
| Fuzz depth | 4 targets (config, pairing bundle, agent HTTP, handshake); PR smoke only | S4 |
| Independent review | P0+P1 surface assessed 2026-11-09; full pre-stable review open | S5 |
| Signed stable release | Not started; needs every gate in §8 | — |

### 3.5 Engine acceptance leftovers

| Engine / mode | Still open |
|---|---|
| `tcp-tls` Direct | `ready` unknown (by design); cross-version update; multi-hour soak; unrelated nft drift on the abroad host |
| `tcp-tls` Reverse (Preview) | Additional evidence gaps remain; any soak is capped at 3600 s; cross-version update and environmental nft drift follow-up remain |
| `noise-tcp` Direct (Experimental) | No loss/soak/ops acceptance; Reverse unavailable. No work planned (L6) |
| `baft` (Preview, Direct only) | Exposed as Preview, Direct only by PR #133 (merged); not configurable (B4); no status/diagnostics (unknown by design); multi-hour soak; listener RSS +23 % over 1200 s (watch in S1) |

### 3.6 Security assessment

SA-01..SA-06, SA-08 and SA-10 are fixed. SA-07 is accepted (the token holder is the host administrator; revisit before any non-loopback management). **SA-09 (separate agent user) is open: do it inside U2**, which creates the agent unit anyway, so the token path and ownership change only once.

### 3.7 Panel traceability gaps (`PANEL_HQ_TRACEABILITY.md`)

| Gap | Status |
|---|---|
| G1 enrolled inventory | Open, L1/L2 |
| G2 end-to-end verification | **Closed** (#109, #120) |
| G3 peer-side preflight | Partly covered by diagnostics after start; peer port ownership open (post-MVP) |
| G4 Apply start/rollback | **Closed**: start (P0-1/P0-5), stop/restart and boot persistence (U3, PR #137, LAB 42692143748 (main `ff868a7`)) |
| G5 audit actor/target | **Closed** (#109) |
| G6 traffic counters | **Closed** (#109, #120) |
| G7 firewall inspection | Open, B3 |
| G8 evidence codes | Partial, B1 |
| G9 BAFT in the panel | Display: done in PR #133; configurable: B4 |
| Physical phone test, formal accessibility audit | Not run, B6 |

### 3.8 Product UX acceptance (D-008) not yet met

- Pairing by a short-lived single-use enrollment code: the MVP uses the bundle + checksum exchange (CLI-tested); an enrollment code needs L1.
- Preview of firewall impact: firewall is Unknown (B3).
- "No manual configuration editing": met for the config itself; ownership, helper and agent steps are still manual (U2).
- Mobile, keyboard and accessibility journeys: headless browser QA only (B6).

### 3.9 Phase 1 leftovers

- Encrypted credential custody needs systemd 250+; not possible on the Ubuntu 22.04 baseline (L5).
- `push-crl.sh` has never run against real hosts (U1).
- CA rollover passed on LAB with ephemeral CAs (42447019410); the push helper was not used in that run (U1 covers one real push).

## 4. Prioritized queue

Effort: **S** = one small PR; **M** = one or two PRs, about a day with CI; **L** = three or more PRs, or an ADR plus LAB, about two to four days. "LAB" means the shared two-host LAB (one run at a time). "Owner" refers to §6.

### Tier Q — quick wins (start now, in parallel, no LAB)

| ID | Work | Why | Effort | Exit evidence |
|---|---|---|---|---|
| Q1 | `phase1-rust` on push to `main`; drop the obsolete branch triggers; weekly `schedule` for `cargo audit`/`cargo deny` and a longer fuzz run | F7: exact-SHA evidence without manual dispatch | S | First push run on `main` green |
| Q2 | Docs truth pass: README, ARCHITECTURE, quickstart en/fa (mark legacy and point to the installed-service path until U8), traceability G2/G5/G6, HQ Issue #2 body §1, `src/baft_engine.rs:6` comment; list the 25 remote branches besides `main` and delete only those whose PRs are merged | F9 | S | PR merged; HQ body refreshed |
| Q3 | Preflight check `directory_writable` (can the agent user create files in `dir`); Blocked with "run the printed setup commands as root"; the panel Apply button follows it | F5: no button that fails on every installed host | S | Unit test + real-systemd CI step |
| Q4 | BAFT stays visible (PR #133) but not selectable ("Preview, not configurable here yet") until B4 | F8 | XS | Browser QA |

### Tier U — blockers to a usable v0.2.0 MVP (in this order)

| ID | Work | Why | Depends | Effort | LAB | Owner | Exit evidence |
|---|---|---|---|---|---|---|---|
| U1 | CRL and certificate operability: `baft-pki.sh renew-crl CA_DIR OUT_DIR [DAYS]`; configurable CRL validity, default 30 days (D6); runbook (renew → `crl-status.sh` → `push-crl.sh`); certificate renewal runbook (90-day certificates); panel CRL warning at 7 days; one real `push-crl.sh` run | F1: tunnels stop after 7 days today | — | S–M | Short run | — | Tests for `renew-crl`; LAB run showing push, CRL number increase, tunnel reconnects |
| U2 | Installed layout and agent service: hardened `baft-tunnel-agent@NAME.service` (non-root, loopback), token created at install, separate agent user (SA-09) in group `baft-tunnel`; `install.sh --instance NAME` registers the instance (control helper, agent unit, directories); `setup init/join --instance` sets owners and modes itself; uninstall, backup, restore and update know the agent unit | F2, F3, SA-09 | — | L | Via U7 | — | Real-systemd CI: agent unit up after install, panel served, no manual `chown`/`chgrp`, uninstall leaves nothing |
| U3 | Lifecycle completeness: Apply `stop` and `restart`; enable at boot through the helper (ADR-0023 amendment, fixed `enable --now` / `disable --now` only); receipt states boot persistence; agent unit enabled at boot | F4 | U2 | M | Via U7 | O1 for a real reboot | Real-systemd CI: `is-enabled`, stop/restart receipts, refusals |
| U5a | Panel lifecycle: Tunnels page Start / Stop / Restart (confirm, receipt), Run diagnostics with one next action per failing tier, boot-persistence state; Wizard last step: the printed setup commands, then Start once the config exists, then diagnostics | Panel integration (owner focus) | U3 | M | Via U7 | O3 phone check | Browser QA against a real agent (fa/en, mobile) |
| U6 | Signed release bundle: `baft-tunnel-<tag>-linux-x86_64.tar.gz` with the binary, scripts, systemd units, PKI helpers and docs, inside `SHA256SUMS` + cosign; install docs: verify, then `sudo ./install.sh`; then prerelease `rc.5` | F6 | U2, U3 | M | — | O8 (release authorization) | Release run; bundle verifies; licence texts and SBOM for the BAFT engine's third-party modules present; `rc.5` published as prerelease |
| U7 | MVP acceptance v2 on LAB with the signed `rc.5` bundle: clean hosts, verified install, setup via the printed commands, agent unit, Apply start/stop/restart through the agent API, diagnostics up to `data_path` on both nodes, 64 KiB + 2 MiB integrity by the stage, `is-enabled`, backup/restore, update from the previous prerelease when layout-compatible (else recorded N/A with the reason), revocation, uninstall including the agent unit, cleanup | Proves the operator journey, not only the engine | U1–U6 | M + LAB | ~60 min | — | `PHASE1_TWO_HOST_E2E=PASS`, new result line, HQ entry |
| U8 | Operator docs for the supported path: quickstart en/fa rewritten (install bundle, PKI, setup, start, diagnostics, CRL and certificate renewal, backup/update, uninstall); troubleshooting by diagnostics tier | MVP promise items 6–7 | U7 | S–M | — | — | Docs follow the U7 run step for step |

**MVP-B, panel-complete setup (recommended right after U8; see D1):**

| ID | Work | Why | Depends | Effort | LAB | Exit evidence |
|---|---|---|---|---|---|---|
| U4 | **Code done** (ADR-0026, real-systemd CI `test-panel-pairing.sh`; the helper derives files from parameters instead of copying staged files). Config write boundary: the agent stages files in its own directory; a root helper (path unit, fixed command) validates them with the same validator as `config validate`, installs them into `/etc/baft-tunnel` with fixed owners and modes, refuses links and overwrites, and writes a result the receipt reads | F5 | U2 | M–L | Via U5b | Real-systemd CI including tamper and symlink refusals |
| U5b | **Done except D2** (ADR-0026, LAB run 42779915532 PASS, browser QA `panel-pairing.js` fa/en): Panel pairing: Apply `init`/`join` from the panel through U4; own bundle export (copy/download with checksum); peer bundle import (paste, upload or link) with checksum compare and preview, which closes P1-8 "Add from Link"; for `tcp-tls`, CSR download and upload of the public certificate, CA and CRL (D2) | Panel integration end to end | U4, U5a | M–L | Yes | Browser QA + LAB run that sets up both nodes through the agent API only |

### Tier B — P1 completion and product quality

| ID | Work | Depends | Effort | LAB | Exit evidence |
|---|---|---|---|---|---|
| B1 | P1-6: per-outcome evidence `code` + `params` in Preflight and diagnostics; panel localizes every line in fa/en; a test fails when a code lacks a translation | — | S–M | — | Unit test; fa screenshots without English evidence text |
| B2 | In-product integrity probe ("Test tunnel"): an authenticated, size- and rate-bounded probe stream answered by the serve node, SHA-256 compared; the `integrity` tier can pass for this probe only; ADR because it touches the stream-open contract (D3) | U7 | M | Yes | Unit + LAB: pass on a good tunnel, fail on a tampered path, never `ready` |
| B3 | P1-7 read-only firewall visibility: a root read helper with a fixed command, filtered to the tunnel port; tier `firewall` pass/fail/unknown; no mutation; ADR | U2 | M | Read-only | LAB shows the tier without changing a rule |
| B4 | BAFT configurable (ADR-0019 phase F): `setup plan/init/join` for `baft` (Direct), Engine Store install in `install.sh`, Apply start/stop, Wizard selectable; status and traffic stay unknown | U5b | L | Yes | LAB with the signed engine through the Wizard path |
| B5 | Bounded persistence of counter and diagnostics history beyond 1 h; panel alerts for CRL/certificate expiry, tunnel down and stale evidence | U2 | M | — | Unit + browser QA |
| B6 | Automated accessibility audit in `scripts/panel-qa` (axe); physical phone pass | U5a | S | — | Report attached to the PR; O3 |

### Tier S — hardening toward Stable (Phase 6)

| ID | Work | Effort | LAB | Owner |
|---|---|---|---|---|
| S1 | Bounded soak up to 3600 s (1 h) per run for `tcp-tls` Direct/Reverse and BAFT listener RSS; no longer soak | M | ≤60 min | — |
| S2 | Cross-version update on every prerelease (N-1 → N), config compatibility included | S per release | Yes | — |
| S3 | Reboot persistence on real hosts, or on disposable VMs (S9) | S | Yes | O1 |
| S4 | Fuzz depth: weekly 10–30 min per target; new targets for the Apply body, bundle import and the runtime evidence file readers | S–M | — | — |
| S5 | Full independent pre-stable security review | — | — | O6 |
| S6 | ~~Licence review of distributing the AGPL BAFT engine~~ Closed 2026-11-09: the owner confirmed BAFT is theirs and its use is allowed, so shipping it as a separate AGPL binary (ADR-0019) needs no further review. Left as a release check in U6: the bundle keeps the engine's source link and the licence texts and SBOM of its third-party modules. Copying BAFT code into BAFT-TUNNEL's own Rust is a separate act: ADR-0019 still asks for the other BAFT contributors' consent ([BAFT_ENGINE_INTEGRATION_DESIGN.md](BAFT_ENGINE_INTEGRATION_DESIGN.md) §15) and its own ADR; nothing in this plan does that | — | — | — |
| S7 | IPv6 evidence (the catalog says `unknown`) if the LAB hosts have IPv6 | S | Yes | — |
| S8 | CRL reload without restarting the unit (today every activation drops connections) | M | Yes | — |
| S9 | Acceptance on fresh VMs instead of cleaned shared hosts | S | Yes | O4 |
| S10 | Find the source of the nftables address-set drift on the abroad host, so firewall neutrality can be claimed | — | — | O2 |

### Tier L — later (P2 and beyond; do not start before Tier U is done)

- **L1** P1-1 enrollment / control-plane ADR and a minimal authenticated remote inventory; single-use enrollment code (D-008).
- **L2** Fleet, groups, remote Jobs with receipts and rollback; remote update (ADR-0025 item 4).
- **L3** More than one tunnel per host in the agent and panel (the agent binds one `--instance` today).
- **L4** Monitoring and notification service; third-party integration API (owner priority).
- **L5** Encrypted credentials (systemd 250+), not available on the Ubuntu 22.04 baseline.
- **L6** `noise-tcp` promotion (loss, soak, ops acceptance) only if the owner asks; Noise Reverse stays unavailable.
- **L7** New transports and rotation (WebSocket/TLS, uTLS, QUIC/UDP, relay): frozen by the owner.

### Dependency summary

```
Q1..Q4 ──────────────────────────────────────────────── (parallel, any time)
U1 ─────────────────────────────────────────────────┐
U2 ── U3 ── U5a ─────────────────────────────────────┤
U2 ── U3 ── U6 (rc.5) ───────────────────────────────┴── U7 (LAB) ── U8 ── MVP-A done
U2 ── U4 ── U5b (needs U5a) ── B4 ── MVP-B done
U7 ── B2 ;  U2 ── B3, B5 ;  S1..S10 after U7
```

Parallel tracks that do not share files or LAB time: (U1, Q1–Q4) alongside U2. U2 → U3 → U5a share `apply.rs`, the control-helper units and the panel, so one thread should own them in order.

## 5. Decisions (owner-delegated authority; recommendation given)

| ID | Question | Recommendation | Why |
|---|---|---|---|
| D1 | Must v0.2.0 set up both nodes entirely from the panel? | **No.** MVP-A: CLI-guided setup (the panel prints the exact root commands) plus panel start/stop/restart and diagnostics. MVP-B (U4, U5b) follows next | Ships a usable product sooner; U4 needs a new privileged helper and an ADR |
| D2 | Default engine and PKI path for the MVP | **`tcp-tls`** (Stable for Direct) with U1 making CRLs operable; `noise-tcp` stays Experimental with no promotion work | Revocation and the strongest evidence are on `tcp-tls` |
| D3 | Is in-product integrity proof part of the MVP? | **No.** The MVP shows the `data_path` tier plus per-release LAB integrity; B2 adds "Test tunnel" right after | B2 changes the stream-open contract and needs its own ADR and LAB |
| D4 | When to fix SA-09 | **Inside U2** | Token path and ownership change once instead of twice |
| D5 | When to make BAFT configurable | **After MVP-A** (B4); until then visible but not selectable (Q4) | Owner focus is P1 and panel integration, not engine work |
| D6 | Default CRL validity | **30 days**, renewed weekly; panel warning at 7 days; `push-crl.sh` keeps refusing under 72 h | Matches the guide's "at least 14 days"; a forgotten weekly renewal no longer breaks the tunnel in one week |
| D7 | Release cadence | Prerelease `rc.5` after U2, U3 and U6; stable only after §8 | Each prerelease carries a cross-version test (S2) |

## 6. Needs the owner's hands

| ID | What | Blocks |
|---|---|---|
| O1 | Approve rebooting the LAB hosts once (or provide disposable VMs) | S3, full U3 proof |
| O2 | Find what changes the nftables address set on the abroad host; prior D2 drift did not reproduce in run 42996224181, but original source remains unknown | S10 (clean firewall claim) |
| O3 | One check of the panel on a real phone through `ssh -L` | B6, U5a sign-off |
| O4 | Fresh VM pair for release acceptance (optional, recommended before Stable) | S9 |
| O5 | ~~Licence review of shipping the AGPL BAFT engine~~ Done 2026-11-09: the owner confirmed BAFT is theirs and its use is allowed (separate-binary model, ADR-0019) | — |
| O6 | External full security review before v1.0 — **paused by owner on 2026-11-11; do not initiate until re-authorized** | S5 |
| O7 | Reserve the LAB hosts for a maximum one-hour soak window if a specific gap requires it | S1 |
| O8 | Release authorization for `rc.5` (delegated to the coordinator under the owner's standing approval; still a separate gate from merge) | U6, U7 |
| O9 | Real offline CA ceremony for any use outside the LAB; the CA key stays with the owner | Real deployments |

## 7. Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| CRL expiry stops `tcp-tls` tunnels after 7 days (F1) | High | High | U1 first |
| Reboot leaves tunnel and agent down (F3, F4) | High | High | U2, U3, S3 |
| Panel Apply fails on installed hosts (F5) | High [INF] | Medium | Q3 now, U4/U5b later |
| Unsigned operator scripts (F6) | Medium | High | U6 |
| Evidence drift, stale docs (F7, F9) | High | Medium | Q1, Q2 |
| One shared LAB pair: collisions, nft drift, not clean VMs | Medium | Medium | One run at a time; S9, S10 |
| Cross-version update never tested | Medium | High | U7, S2 |
| No multi-hour soak (BAFT listener RSS +23 % in 1200 s) | Medium | Medium | S1 |
| Agent DoS by a compromised tunnel (SA-09) | Low | Medium | U2 |
| Third-party licences inside the BAFT engine (BAFT itself is the owner's, confirmed 2026-11-09) | Low | Low | Release keeps their licence texts and SBOM (U6) |
| Scope creep (new engines, Fleet) before the MVP | Medium | High | Tier L frozen until Tier U is done |

## 8. Release gate status (Roadmap §7, as of this baseline)

| Gate | Status |
|---|---|
| P0-1..P0-5 with exact SHA and CI/LAB evidence | Met |
| Catalogue, CLI, Preflight, Wizard and runtime agree | Partly: BAFT selectable-then-blocked since #133 (Q4); panel Apply vs installed layout (Q3, U4) |
| Reverse Preview and Noise Reverse unavailability truthful | Met |
| BAFT Store path uses the pinned artifact and manifest; licence package verified | Artifact met (A1–A3); licence: the owner owns BAFT and allows its use (2026-11-09), attribution package checked in U6 |
| Apply receipts disclose what happened and what is unverified | Met |
| Signed asset, checksum, signature, manifest, rollback verified | Binary met (rc.4, P0-5); scripts and units unsigned (U6) |
| Upgrade/restore/uninstall on baseline hosts | Same version met (LAB 42635190485, 42619848756); cross-version open (U7, S2) |
| No unresolved Critical/High security or cleanup issue | Met (SA-07 accepted, SA-09 open Low) |
| Independent review before a stable v1.0 claim | P0+P1 surface only (S5) |
| HQ Issue #2, Roadmap, ADRs, evidence updated | Roadmap and board updated with this plan; HQ body refresh in Q2 |
| Release authorization separate from merge | Process in place |

## 9. Kept out of scope

No new engines or protocols, no Fleet or billing, no React migration, no `noise-tcp` promotion, no firewall mutation, no change to production hosts. Nothing in this plan authorizes a tag, release or deploy; those stay separate gates.

## Historical progress snapshot 2026-11-09: U2, U3, U5a (superseded by the verified status correction at the top)

- Merged: PR #137 (`8d4b1f4`, agent service, lifecycle, panel), #140 (LAB stage), #141 (Wizard last step), #142 (LAB package contents).
- LAB run 42692143748 on `main` `ff868a7`, both hosts Ubuntu 22.04 / systemd 249: clean hosts; one-step `install.sh --binary --register`; setup as root with no manual `chown`; agent service started by itself as `baft-tunnel-agent` on loopback; Start (enabled at boot), Restart (new process), Stop (disabled, no data), Start again through `POST /v1/tunnel/lifecycle`; 64 KiB + 2 MiB integrity; diagnostics `next_action` none while running and start while stopped; **one real reboot of both hosts** (owner-approved, O1): back over SSH in about 44 s, tunnel and agent active and enabled with no command from the test, 64 KiB + 2 MiB integrity after the reboot; full uninstall, users removed, no host drift.
- Earlier runs 42684792291 and 42687963213 stopped on LAB script defects (fixed in #141, #142); both cleaned up.
- U2 and U3 done; U5a done except the O3 phone check. Still open: U6 (`rc.5`) and the rest of the queue; receipt `ready` stays Unknown by design. Not production-ready.

## Historical progress snapshot 2026-11-10: D2-B1/B2/C local implementation candidate (superseded by PR #150 merge and LAB run 42996224181)

- Branch: feature/d2-panel-tcp-tls-enrollment; issue #149; PR #150 remains Draft while runtime implementation and acceptance evidence are completed.
- Added the separate root-only CSR create/read protocol (NAME.pki / NAME.pki-result), digest-bound replay receipts, P-256/CN/key-match validation, fixed PKI paths, no-overwrite installation, and node/SAN/public-key metadata. The helper returns only public CSR material; the private key remains at /etc/baft-tunnel/node.key with baft-tunnel ownership and mode 0600.
- Added hardened PKI systemd path/service templates and staged installer registration/removal coverage.
- Added Agent-side atomic request publication, strict root receipt ownership/mode checks, fixed config-directory binding, and authenticated POST /v1/pki/csr/create and POST /v1/pki/csr/read endpoints. Create is confirm-gated and pre-Init only; configured mode permits read only for the matching tcp-tls node. Setup state, node ID, DNS/IP SAN, request size/ID, receipt and key-custody flag are checked; all outcomes are audited without PEM or key material.
- Local Mac evidence: 16 PKI-related Rust tests passed; bash scripts/test-install.sh passed; cargo clippy --locked --all-targets -- -D warnings passed; cargo fmt --all -- --check and git diff --check passed. Serial full suite (cargo test --locked --no-fail-fast -- --test-threads=1): 167 passed, 4 failed. The same four host-sensitive failures were present on the unchanged baseline before this work: agent diagnostics fixture; two engine-store fixtures expecting a linux-amd64 demo artifact; observe session-owner evidence. No test failure is represented as a pass.
- Scope remains incomplete: no panel UI, certificate/CA/CRL import, Linux host validation, exact-head CI for the latest code, or two-host LAB. D2 remains OPEN / NOT VERIFIED; tcp-tls stays unavailable in the Wizard until all gates pass.
