# Reference projects review (consolidated)

Status: consolidated review, 2026-11-17, `main` `b7172bc`. **Documentation only. Not a decision record, not permission to change code, tag, release or deploy.**
This file consolidates, and does not replace: [RAKHSH_INTEGRATION_ANALYSIS.md](RAKHSH_INTEGRATION_ANALYSIS.md), [ADVISOR_ASSESSMENT.md](ADVISOR_ASSESSMENT.md), [research/ENGINE_ECOSYSTEM_COMPARISON.md](research/ENGINE_ECOSYSTEM_COMPARISON.md) (and its siblings), [CROSS_PROJECT_CAPABILITY_ALIGNMENT.md](CROSS_PROJECT_CAPABILITY_ALIGNMENT.md), ADR-0019, ADR-0038 to ADR-0049. Where a source document has the detail, this file links to it instead of repeating it.

## 0. How to read this file

Every statement carries one of four labels.

| Label | Meaning |
|---|---|
| **Verified** | Read in a repository file, PR, release or run log on 2026-11-17, or recorded in a cited merged document. |
| **Historical** | A decision or finding recorded earlier; still binding unless a later record changes it. May be stale. |
| **Proposed** | A suggestion in this file or in an advisory document. Not decided, not implemented. |
| **Unverified** | Could not be confirmed from repository records. Stated as such; nothing is invented to fill the gap. |

Rules used throughout:

- An **accepted ADR is not an implemented feature.** Implementation status comes from code on `main`, tests and runs, never from an ADR title.
- Evidence levels stay separate: **CI ≠ simulated/hosted netns ≠ single-host ≠ two-host LAB ≠ Production**. Nothing here is production evidence.
- Reference-project facts come from each project's own README/LICENSE or from read-only source review in the cited document. They are claims of those projects, not measured behaviour. **Nothing was executed or benchmarked for this file.**
- URLs appear only where a repository document records them. No URL was guessed.

## 1. Executive summary

1. **Reference set (Verified, with caveats).** Eleven-plus projects were reviewed across five documents: Rakhsh, BackPack, C-Tunnel (and the owner's fork C-BAFT), rathole, FRP, Chisel, spoof-tunnel, TAQ-BOSTAN, Phormal, Pahlavi-tunnel, rathole-tunnel (ecosystem study, 2026-11-08); Rakhsh in depth (2026-11-17); and Hashem Panel, Pangolin, Tailscale as README-level advisory comparisons (2026-11-17). Identities and URLs are in §2. The mission's "Backpack" is **BackPack by AminMGMT** (`AminMGMT/BackPack`), recorded in the ecosystem study; it is a different project from Backhaul, which is **not** among the recorded reviews.
2. **Licence position (Historical, binding).** Rakhsh, BackPack, C-Tunnel/C-BAFT and Hashem Panel are AGPL-3.0; the BAFT-TUNNEL Rust core is proprietary (ADR-0019 clause 5). No reference code is in the core. Rakhsh ideas are independently rewritten (ADR-0038); copying is allowed only into an isolated AGPL unit (ADR-0043) and **nothing has been copied**. Licences were re-read from the cited documents, not re-fetched from the upstream repositories in this pass (see §3 and open question OQ-1).
3. **What was adopted is mostly design plus pure unwired modules, not product behaviour.** Verified on `main`: `backup_remotes` failover is wired into the dial path (#296); `doctor` offline CLI (#298); health publishing (#306, quality stays Unknown without samples); optional Direct session rotation (#315, ADR-0049, default off). Pure and **not wired**: link policy (#292), transport health model (#293), resume core (#309). Design only: stream resume protocol v3 (ADR-0040), UDP carrier (ADR-0046), Fleet (ADR-0045). The README-level reference matrix in §12 gives status per item.
4. **The strongest installation reference is C-Tunnel-style one-line setup**; BAFT-TUNNEL already ships a signed one-liner, HTTPS panel and guided Wizard. The remaining UX gap versus the references is a **single join link for the second host** and a **path test before building** (R-7, R-8 in the advisor file).
5. **Largest open reliability item is not a reference lesson but a measured path problem:** aged TCP flows on the Iran path are throttled (§11). A partial mitigation (ADR-0049 session rotation) is merged, default off. The owner reports a real two-host LAB run on **Noise-TCP Direct** (off 0/3, rotation 10 s 3/3, run 44121043530, see §9); that is **not TCP-TLS evidence**, and the aged-flow cause is not established.
6. **Roadmap documents were out of date** on release state (they said Stable 0.2.1 was latest; Stable `v0.2.2` was published 2026-11-17 15:39Z on `5dd0890`). Corrected in this PR (§14).

## 2. Reference repository identities

URLs are those recorded in repository documents. "—" means the repository records no URL and none is supplied here.

| # | Reference | Identity / URL | Where recorded | Reviewed at | Depth | Identity status |
|---|---|---|---|---|---|---|
| 1 | Rakhsh | `https://github.com/CluvexStudio/rakhsh` (copyright holder `CluvexStudio`); wire protocol in a separate repo named `shabdiz` (path dependency) | RAKHSH_INTEGRATION_ANALYSIS | `89fa8e131f5d423fb709a9c9c3c28cbc0cee39cd` (2026-11-12), read-only; bodies of `carrier.rs`, `links.rs`, `resume.rs`, `doctor.rs` deliberately **not** opened | Layout, sizes, tests names, user docs | Verified. `shabdiz` URL/licence **Unverified** (not reviewed) |
| 2 | BackPack | `https://github.com/AminMGMT/BackPack` | ENGINE_ECOSYSTEM_COMPARISON §1 | HEAD `9d58f8449ac0`, tag `v1.9.0` | Read-only source clone, nothing executed | Verified. Not Backhaul, not Rakhsh, not Hashem |
| 3 | C-Tunnel | `https://github.com/CIAUB/C-Tunnel` (AGPL-3.0, NOTICE attribution + trademark) | ENGINE_ECOSYSTEM_COMPARISON §2.1 | upstream `7fe07e8` audited by the fork | Via the fork only | Verified as the upstream of C-BAFT. Upstream was not cloned separately |
| 3a | C-BAFT (module `ctunnel`; the "ctunnel" of earlier discussion) | `https://github.com/zarkmakerburg/C-BAFT` | ENGINE_ECOSYSTEM_COMPARISON §1 | HEAD `ed576396519f` (2026-10-30) | Source read, nothing executed | Verified. Owner-controlled **fork of third-party code**; lineage BackPack → C-Tunnel → C-BAFT. A repository `M-R-R-X/ctunnel` was tried and is **not found/unverified; not used** |
| 4 | rathole | `https://github.com/rathole-org/rathole` | ENGINE_ECOSYSTEM_COMPARISON §1 | HEAD `a292f7ed5402`, `v0.5.0` | Source read | Verified. Apache-2.0, Rust |
| 5 | FRP | `https://github.com/fatedier/frp` | same | HEAD `d42812ae0eb6`, `v0.72.0` | Source read | Verified. Apache-2.0, Go |
| 6 | Chisel | `https://github.com/jpillora/chisel` | same | HEAD `5ee6c74d60d9`, `v1.13.0` | Source read | Verified. MIT, Go |
| 7 | spoof-tunnel | `https://github.com/ParsaKSH/spoof-tunnel` | same | HEAD `ba29f0c8e22d`, `v3.1.0-beta.0` | Source read | Verified. Apache-2.0; rejected (§10) |
| 8 | TAQ-BOSTAN | `https://github.com/ParsaKSH/TAQ-BOSTAN` | same | HEAD `264cec99b23a`, `v2.1.0` | Scripts read | Verified. Apache-2.0; ideas only |
| 9 | Phormal | `https://github.com/Schmi7zz/Phormal` | same | HEAD `65090c2e820d` | One 6,459-line bash script | Verified. GPL-3.0; ideas only (Path Test) |
| 10 | Pahlavi-tunnel | `https://github.com/Zehnovik/Pahlavi-tunnel` | same | HEAD `00191506e7c1` | Source read | Verified. **No licence file, no authentication**; not used |
| 11 | rathole-tunnel | `https://github.com/Musixal/rathole-tunnel` | same | HEAD `ed49cdd88945` | Scripts read | Verified. GPL-3.0; committed unverifiable binaries; not used |
| 12 | Hashem Panel | — (no URL recorded in repository documents reviewed) | ADVISOR_ASSESSMENT | README-level only | README claims | Identity beyond name/licence **Unverified** here. AGPL-3.0 per the advisor file. The owner's earlier mission notes list Hashem as a panel pattern reference (PRODUCT_ROADMAP §1) |
| 13 | Pangolin | — (no URL recorded) | ADVISOR_ASSESSMENT | README-level | README claims | **Unverified** beyond name. Mixed AGPL-3.0 / commercial per-file headers per the advisor file |
| 14 | Tailscale | — (no URL recorded) | ADVISOR_ASSESSMENT | README-level | Not assessed in depth | **Dropped as a reference by the owner on 2026-11-17** (not needed; other tunnel projects are the useful references). Kept in the older documents for traceability only |
| 15 | BAFT (owner's own engine) | `zarkmakerburg/BAFT` (not the product repo) | ENGINE_ECOSYSTEM_COMPARISON §2.1 | `2acb495eafe5` | Used as a separate pinned engine process (ADR-0019) | Verified. **Not a reference to copy from**; never merge into this repository |

**Not found in any repository record:** a review of Backhaul; any Backpack finding that predates the 2026-11-08 ecosystem study; any other earlier-evaluated repository beyond the table. Earlier conversations outside the repository were not available to this thread, so reviews that exist only there are **Unverified/unrecoverable here**.

### 2a. Additional tunnel references (added 2026-11-17 on the owner's instruction)

The owner said Tailscale is not needed and that other tunnel projects should be mined for ideas. Candidates below were looked up through GitHub repository search on 2026-11-17, which returned the exact name, licence tag, main language and last push. **Only those facts are verified.** No README or source of these projects was read in this pass, so every "idea" is **Proposed from the project's category, not from its code**, and must be checked against the README/source before any ADR cites it. Licence tags are GitHub's detected SPDX values, not a legal review. Independent implementation only; no code is copied (ADR-0019, ADR-0038).

| Project | Repository | Language | Licence tag | Last push | Candidate idea (Proposed, unread) | Status |
|---|---|---|---|---|---|---|
| Backhaul | `Musixal/Backhaul` (885 stars) | Go | AGPL-3.0 | 2025-09-04 | Reverse tunnel with several selectable transports and a multiplexed variant; same author as `Musixal/rathole-tunnel` in §2. Compare with BackPack and our Reverse Preview | **Studied (README only), see §2b**; ideas only (AGPL) |
| EasyTier | `EasyTier/EasyTier` (14.8k) | Rust | LGPL-3.0 | 2026-11-17 | Peer-to-peer mesh VPN in Rust (multi-peer, NAT traversal). Relevant to Fleet/multi-node thinking only | **Deferred**; LGPL does not permit copying into a proprietary core without compliance work |
| wstunnel | `erebe/wstunnel` (7.2k) | Rust | BSD-3-Clause | 2026-10-19 | TCP/UDP carried over WebSocket/HTTP; a model for a future WS carrier (ADR-0010 plans carriers each with its own ADR) | **Studied (README only), see §2b**; WS carrier not built |
| bore | `ekzhang/bore` (11.6k) | Rust | MIT | 2026-02-04 | Minimal reverse TCP tunnel with a shared-secret handshake; a small readable reference for the Reverse control/data split | **Studied (README only), see §2b** |
| Ehco | `Ehco1996/ehco` (1.3k) | Go | GPL-3.0 | 2026-11-08 | TCP/UDP relay with WS/WSS/mux transports and traffic metrics | **Deferred**; ideas only (GPL) |
| nps | `ehang-io/nps` (34k) | Go | GPL-3.0 | 2024-05-30 | Intranet-penetration proxy with a web console and multi-client management; a product/panel reference | **Deferred**; ideas only (GPL); not pushed since 2024 |
| sing-box | `SagerNet/sing-box` (40k) | Go | `NOASSERTION` (custom/unclear) | 2026-11-17 | Universal proxy platform | **Not assessed**; licence tag unclear, so no idea is adopted until the licence is read |

### 2b. README study of Backhaul, wstunnel and bore (2026-11-17, coordinator order)

Method: each project's `README.md` on its default branch was read (Backhaul and wstunnel on `master`, bore on `master`); no source and no architecture document beyond the README was read, and nothing was run. Claims below are the **projects' own README statements**, not verified behaviour. All ideas are **Proposed**, independent rewrite only, mapped to existing roadmap/advisor items.

**Backhaul** (AGPL-3.0 per its README License section, matching the GitHub tag). Reverse tunnel in Go. README states: transports tcp, tcpmux, ws, wss, wsmux, wssmux (SMUX multiplexing, tunable frame and buffer sizes); UDP carried over the TCP transport; one shared `token` for authentication (optional); keepalive and `retry_interval`/`dial_timeout` settings; a server-side `channel_size` queue where **excess connections are discarded**; optional traffic "sniffer" log and a `web_port` monitoring page; hot configuration reload. A separate benchmark page exists and was **not** read.
- Idea B-1 (Proposed): a **bounded accept queue with explicit overflow policy** surfaced as a counter. Maps to the Reverse ingress-queue deadline gap already noted in the ecosystem study; roadmap O6/ADR-0037 follow-up.
- Idea B-2 (Proposed): a **transport-choice FAQ in the Wizard** (plain TCP vs multiplexed vs HTTP-friendly), shown only for carriers that actually exist. Maps to R-8 (doctor/path test) and to the rule "never advertise unsupported carriers".
- Rejected: token-only identity (weaker than ADR-0011/0012), plaintext `ws` option, built-in traffic sniffer log (privacy and scope).
- Gap: the README gives no recovery/resume semantics beyond retry; nothing to adopt for R-3.

**wstunnel** (BSD-3-Clause per GitHub tag; Rust). README states: tunnels TCP/UDP/Unix/stdio and SOCKS5/HTTP/transparent proxy over WebSocket, HTTP/2 or WebTransport; reverse tunnelling; **mTLS with certificate auto-reload**; a restrictable HTTP-upgrade **path prefix as a shared secret** and a restriction config file; websocket ping frequency; reverse client reconnect with a configurable maximum backoff (default retry every 1 s); proxy-protocol header support; TLS certificate verification is **disabled by default on the client**.
- Idea W-1 (Proposed): **carrier plug-in shape** for a future WS/HTTP carrier (ADR-0010 requires its own ADR, audit and LAB); useful as a design comparison for ADR-0039's carrier interface. Roadmap Phase 4, after R-1/R-2.
- Idea W-2 (Proposed): **configurable max reconnect backoff exposed in config** with a documented default. Maps to ADR-0039 link policy; our 1 s to 60 s doubling is hard-coded per docs.
- Idea W-3 (Proposed): **restriction rules per tunnel** (allowed destinations) as a declarative file. Overlaps ADR-0011 authorization; check before proposing anything new.
- Rejected: verification off by default (violates fail-closed), path-prefix-as-secret as authentication, transparent proxy (needs privileges, ADR-0030).
- Note: HTTP/2 and WebTransport carry operational caveats in the README (proxy buffering, UDP reachability). No evidence for the Iran path; **no censorship-resistance claim**.

**bore** (MIT per GitHub tag; Rust, README says about 400 lines). README states: control port 7835, a Hello message, a per-connection UUID, a second TCP stream per accepted connection, **incoming connections held at most 10 seconds if the client does not accept them**, optional shared secret verified by challenge on each connection.
- Idea R-1 (Proposed): the **10 s accept-hold limit** is the same fix as B-1; prefer one ADR-0037 amendment covering both.
- Idea R-2 (Proposed): **challenge-per-connection** is already stronger than a static token but weaker than our mTLS; no action.
- Rejected: shared-secret-only deployment model; nothing else to adopt (it is deliberately minimal).

**Summary for the roadmap:** the three READMEs add no new capability that BAFT-TUNNEL lacks; they confirm two small hardening items (bounded Reverse accept queue with a deadline; configurable reconnect backoff) and one future design comparison (WS/HTTP carrier shape). Neither is scheduled by this document.

Searched and **not confirmed**: `go-gost/gost` and `apernet/hysteria` returned no result for the exact search used; neither is listed. Direct repository metadata for these projects could not be read from this session (access is scoped to the product repositories), so no further fact beyond the table is claimed.

## 3. Licensing comparison

Source for all rows: the cited review documents (Historical, from each project's `LICENSE`/`NOTICE` at the pinned commit). Not re-fetched in this pass. **Not legal advice**; counsel review is advised before any decision to copy code or relicense.

| Reference | Licence | Consequence for a proprietary Rust core (ADR-0019 cl. 5) |
|---|---|---|
| Rakhsh | AGPL-3.0-only | Ideas only. ADR-0038 clean-room; ADR-0043 copy only into an isolated AGPL unit; nothing copied |
| BackPack | AGPL-3.0 + NOTICE s.7 (attribution/trademark) | Separate-process only; ideas only for core; engine is reviewed as a candidate **Go** engine, never copied into Rust |
| C-Tunnel / C-BAFT | AGPL-3.0 + NOTICE; fork of CIAUB code with BackPack-ported files | Owner controls the repo but not the copyright; separate process or ideas only |
| Hashem Panel | AGPL-3.0 (advisor file) | Ideas only |
| Pangolin | Mixed AGPL / commercial (advisor file) | Ideas only; legal review before any reuse |
| rathole | Apache-2.0 | Code reuse legally possible with notices; policy still design-only, re-implement against the authenticated session |
| FRP | Apache-2.0 | Go; ideas / separate process only |
| Chisel | MIT | Go; ideas only. Cleanest Go licence |
| spoof-tunnel | Apache-2.0 | Rejected on risk, not licence |
| TAQ-BOSTAN | Apache-2.0 | Scripts, ideas only |
| Phormal, rathole-tunnel | GPL-3.0 | Ideas only |
| Pahlavi-tunnel | none | Do not use |

## 4. Individual technical reviews (summary; detail in the linked documents)

### 4.1 Rakhsh (Rust, AGPL-3.0-only)

- **Verified architecture:** single Rust binary plus a separate control panel/bot; carriers tcp/tls/ws/wss/quic/quicv2/kcp plus privileged raw carriers (faketcp, l2tcp, icmp, spoof); links manager, stream resume (offset/cumulative ACK, 16 MB window), doctor (probe each carrier, rank by speed), status (per-lane RTT/jitter/loss), limits/quota, DNS/route/proxy, SSH+Telegram fleet, Noise handshake, an `encryption = "none"` mode.
- **Strengths (per user docs, not executed):** multi-carrier failover/failback, resume, per-lane health, doctor, `--join` link UX.
- **Weaknesses / risks:** privileged raw/L3 modes, plaintext mode, SSH/bot custody, wire protocol in an unreviewed separate repo of unknown licence, proxy/router scope.
- **Security implications for BAFT-TUNNEL:** any adopted idea adds attack surface (more dials, a second accept path, resume buffers); mitigations are named in RAKHSH §9 and ADR-0039/0040.
- **Detail:** [RAKHSH_INTEGRATION_ANALYSIS.md](RAKHSH_INTEGRATION_ANALYSIS.md).

### 4.2 BackPack (Go, AGPL-3.0 + NOTICE s.7)

- **Verified architecture (source read, nothing executed):** one binary, three engines (reverse, direct, layer-3) by config table; one process and one systemd unit per tunnel; reverse has 12 transports (tcp, tcpmux, stealth, ws, wss, wsmux, wssmux, kcp, quic, udp, xdi, pck), a signal-byte control channel, a pre-opened data-connection pool, and a "generation" lifecycle that survives client re-dials; same-list transport fallback with dwell-based rendezvous; management menu/web panel/Telegram/monitor/SSH fleet in the same binary.
- **Strengths:** reconnect lifecycle with tests, fallback chain, process-per-tunnel with a local status socket, large test base (481 `_test.go` files at the pinned commit).
- **Weaknesses:** shared-token authentication only (no per-peer identity, no CRL); TLS 1.3 floor enforced only in the l3 QUIC path; dialing role exposes no local listener, so TCP-accept readiness does not apply.
- **Selected / rejected / deferred:** see §11 and the matrix. Candidate **Go engine** only after three gates (identity model, readiness-contract extension, AGPL obligations accepted); BAFT (owner's engine, ADR-0019) is the engine that actually shipped as Preview.
- **Detail:** ENGINE_ECOSYSTEM_COMPARISON §2, ENGINE_INTEGRATION_FEASIBILITY, ENGINE_LICENSE_AND_DEPENDENCY_MATRIX.

### 4.3 C-Tunnel / C-BAFT (Go, stdlib only, AGPL-3.0)

- **Verified:** 28 methods (13 kernel L3, 7 L2, 8 userspace UDP-Crypt: dtls-1.3, quic-v1 look-alike, rudp-fec), setup by a `ctunnel://` link, per-tunnel unit plus panel plus watchdog; authentication by one shared secret (AES-256-GCM with HMAC-derived subkeys and a replay window); kernel methods configured through `ip`/`iptables` helpers (host-mutating).
- **Fit:** **not** an engine candidate (host network mutation, shared secret, third-party lineage). Value is product ideas: one-line/link-based provisioning, a Connection Test that sends real probes and reports measured PASS/UNSTABLE/BLOCKED with measured MTU, `rudp-fec` (idea only).
- **Reverse claims** in its README are **D** (documented) at best; the handshake of its carrier was not traced.

### 4.4 rathole (Rust, Apache-2.0) · FRP (Go) · Chisel (Go)

- **rathole:** ~3.1k LOC; control channel plus one fresh TCP data connection per user flow; tcp/tls/noise/websocket; hot reload; UDP forwarding. Data-channel hello carries a session digest in clear on plain TCP (bearer). **Best Reverse design reference.**
- **FRP:** mature; control connection plus work-connection pool; yamux default on; tcp/kcp/quic/websocket/wss; TLS default on `frpc`; `runID` replaces a stale control on reconnect; dashboard, Prometheus, health checks. **Best reference for session identity and reconnect-replace semantics.**
- **Chisel:** HTTP/WebSocket carrying an SSH session; host-key fingerprint pin, per-user permissions, auto-reconnect with backoff, application ping. **Cleanest Go licence, simplest model.**

### 4.5 Others

- spoof-tunnel: **rejected** (forged source addresses, `CAP_NET_RAW`/XDP, no payload crypto in the v2+ core).
- TAQ-BOSTAN / Phormal: scripts; **ideas only** (Phormal Path Test; its "best choice" is a hard-coded rank, not a measurement).
- Pahlavi-tunnel / rathole-tunnel: **not used** (no auth / no licence; unverifiable binaries).
- Hashem Panel / Pangolin / Tailscale: README-level comparison only in ADVISOR_ASSESSMENT §2; no source read. Hashem: carrier swap with auto-revert and a path test, central panel. Pangolin: RBAC per resource. Tailscale: not assessed in depth.

## 5. Product and UX comparison

Source: ADVISOR_ASSESSMENT §6 (README-level) and the roadmap. "Verified" = BAFT-TUNNEL column read from roadmap/evidence docs.

| Step | BAFT-TUNNEL (Verified) | Best reference pattern (reference README, unverified) | Gap (Proposed) |
|---|---|---|---|
| Install | Signed one-line install pinned to a tag, offline signature verification (ADR-0034), `--domain`/`--ip` HTTPS, interactive menu | Rakhsh: one line asks a few questions; C-Tunnel: one-line setup | None material. README pin must follow each Stable tag |
| Connect | Pairing bundle + checksum, panel-managed enrollment | Rakhsh `--join rakhsh://…`; C-Tunnel `ctunnel://` link; Hashem one setup bundle | **No single expiring join link for the second host** (advisor R-7) |
| Create tunnel | Guided Wizard (Direct; Reverse Preview; BAFT Preview Direct only) | Hashem path test before building; Phormal/C-BAFT Connection Test | **No carrier path test**; `doctor` is offline only (#298) |
| Understand state | Honest Unknown/stale states, diagnostics tiers, integrity probe | Hashem single health model, webhook/Telegram alerts | Health publishing exists (#306) but quality stays Unknown; no notification channel |
| Operate | Backup/restore, guarded update with rollback, update notice (ADR-0035) | Hashem Autopilot / carrier swap with auto-revert | Auto-swap is **not accepted** without evidence (R-3) |
| Troubleshoot | Troubleshooting doc, support bundle (#272) | — | — |

## 6. Architecture comparison

| Aspect | BAFT-TUNNEL (Verified) | Reference approach | Note |
|---|---|---|---|
| Core | Rust, tokio, one yamux session per tunnel over one authenticated stream | Rakhsh lanes; FRP pool + mux; BackPack generations; rathole one TCP per flow | Single-session head-of-line coupling is not covered by an ADR (advisor R-9, **Proposed research**) |
| Engines | `tcp-tls` (Direct Stable, Reverse Preview), `noise-tcp` (Experimental, Direct), `baft` (separate Go process, Preview Direct only) | BackPack/FRP/Chisel are whole engines | Core stays Rust; Go only as a separate pinned process (ADR-0019) |
| Direct vs Reverse | Detected before engine selection; Reverse Preview only for `tcp-tls`; Noise Reverse unavailable | rathole/FRP reverse-first | Reverse not public until two-host acceptance (fixed constraint) |
| Control plane | Local agent over UDS, panel, no central plane | Hashem/Pangolin/Rakhsh fleets; FRP dashboard | Fleet = ADR-0045 accepted with conditions, F1 core rules draft #326; **no control plane** |
| Privileged networking | None; `CAP_NET_ADMIN` rejected (ADR-0030) | Rakhsh raw/L3; C-BAFT kernel L3; spoof-tunnel raw | Product position, not a gap |

## 7. Transport comparison

| Transport | BAFT-TUNNEL | Rakhsh | BackPack | FRP | rathole | Chisel |
|---|---|---|---|---|---|---|
| TCP + TLS 1.3 mTLS | **Implemented, Stable (Direct)** | D | I (partial floor) | I | I (build-dependent) | I (optional) |
| Noise over TCP | **Implemented, Experimental (Direct)** | D | V (`stealth`) | N | V | N |
| WebSocket / WSS | Not implemented/advertised | D | I | V | I | V (only transport) |
| QUIC | Not implemented (planned later, own ADR) | D | I | I | N | N |
| KCP / FEC | Not implemented | D | I | I | N | N |
| UDP forwarding | **Not implemented** (ADR-0046 design, #244) | D | I | V | V | V |
| Multiplexing | yamux | per-lane | smux variants | yamux | none | SSH channels |

Legend (from the ecosystem study): V = source read and tests exist upstream (not run by us), I = source read, no test located, D = documented only, N = absent. "Not implemented" for BAFT-TUNNEL means **absent from `/v1/engines`, Wizard and docs advertisement**; never advertise it until built and accepted.

## 8. Security comparison

| Aspect | BAFT-TUNNEL (Verified from docs) | References (reviewed docs) |
|---|---|---|
| Peer identity | Per-node mTLS or Noise static key; SHA-256 pins; CRL (offline PKI) | Shared token: BackPack, rathole (per service), FRP (token or OIDC; optional mTLS), C-BAFT (shared secret); Chisel SSH keys; Rakhsh per-node keys |
| Revocation | CRL with reload for new connections (S8) | Not found in the reviewed references except Chisel permission regexes |
| Plaintext path | None | Rakhsh `encryption = "none"`; spoof-tunnel core has no payload crypto |
| Privilege | Non-root hardened systemd units; no `CAP_NET_ADMIN` | Raw/L3/kernel helper modes in Rakhsh, C-BAFT, spoof-tunnel |
| Assessments | P0+P1 assessed 2026-11-09 (3 Medium, 3 Low fixed); pre-Stable audit with no Critical/High/Medium; **full independent external review paused by owner** | None of the references' own audits were reviewed |

## 9. Reliability and recovery comparison

| Capability | BAFT-TUNNEL | Evidence level |
|---|---|---|
| Reconnect with backoff (1 s to 60 s doubling), session-level only | Implemented | Two-host LAB (restart/kill recovery 2–4 s; 30 s blackhole 8 s for Reverse R3) |
| Backup endpoints (`backup_remotes`) in the dial path | Merged (#296, ADR-0039) | CI only; **no LAB run of an actual failover recorded** |
| Link policy module (backoff, failback) | Merged, pure, **unwired** (#292) | Unit tests only |
| Transport health model | Merged, pure, **unwired** (#293); status publishing (#306) shows quality Unknown without samples | Unit tests only; no RTT/probe source |
| Stream resume | Pure core merged, **unwired** (#309); protocol v3 design (ADR-0040) | Unit tests only |
| Session rotation (make before break) for aged-flow throttling | Merged (#315, ADR-0049), config `session_rotate_secs`, **default off**; a single long user connection is still throttled after about 20 s | CI test; **Noise-TCP Direct two-host LAB** (owner-reported, run [44121043530](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44121043530)): rotation off 0/3 complete, `session_rotate_secs=10` 3/3, 12 rotation events, `ROT_ACCEPTANCE=PASS`. Not TCP-TLS; not Production; the cause is not established |
| Competitive fault test (blackhole 5 s x5, 2 s x3 per tool) | Harness on a hosted-runner netns job (#330 merged; #308 per the roadmap, state not re-checked) | **Hosted-runner simulation, not LAB**. Result text is in CI logs, not summarized in a repository document; the single earlier run reported BAFT truncated while FRP and rathole were correct (#330 body). Treat as an unconfirmed observation |

Reference side (README/source claims, unverified): Rakhsh per-lane resume and failback; Hashem carrier swap with 45 s auto-revert; BackPack generation lifecycle with tests; FRP loginLoop with FastBackoff and runID replace; Chisel keepalive plus backoff.

## 10. Installation and operational comparison (summary)

Covered by §5. Operational features adopted so far: backup/restore, guarded update + rollback, offline `doctor`, support bundle. Not adopted: SSH fleet, Telegram/bot control, auto carrier swap, general proxy/DNS routing, quota (design only).

## 11. Selected, rejected and deferred ideas

**Selected (adopt as independent implementation):**
S-1 carrier link policy (failover, backoff, failback) — Rakhsh/Hashem/BackPack fallback chain; S-2 transport health model — Rakhsh/FRP; S-3 stream resume — Rakhsh; S-4 offline doctor and config schema — Rakhsh; S-5 control channel + on-demand data stream structure and `runID`-style stale-session replacement — rathole/FRP (for Reverse, ADR-0021); S-6 process-per-tunnel with a local status socket — BackPack; S-7 real-traffic Connection/Path Test — Phormal/BackPack/C-BAFT (design, not built); S-8 one-line link provisioning — Rakhsh/C-Tunnel (join link not built); S-9 adaptive traffic policy (userspace only) — Rakhsh/Hashem ideas, ADR-0031/0044.

**Rejected:** raw/faketcp/icmp/spoof/L3 and `CAP_NET_ADMIN` (owner ban, ADR-0030); plaintext mode; SSH-based fleet and Telegram bot custody (ADR-0013/0014); general DNS/proxy router mode; copying any reference code into the core (ADR-0019/0038); depending on `shabdiz`; shared-token as sole identity; forged-source carriers; unauthenticated tools; committed third-party binaries.

**Deferred:** QUIC/WS/KCP carriers (each its own ADR and audit); UDP/datagram carrier (ADR-0046, #244; prerequisite only for Gaming/Trading benefit claims); central Fleet (ADR-0045, F1 pure rules in draft #326); quota/bandwidth limits; webhook notifications; head-of-line blocking research; BackPack as a Go engine candidate pending its three gates.

## 12. Comparison matrix

Status vocabulary: **Implemented** (code on `main`, evidence stated), **Partial**, **Merged unwired** (pure module, not on the data path), **Designed only** (ADR or doc, no code), **Rejected**, **Deferred**, **Unverified**.

| Reference | Area | Selected lesson | Rejected approach | BAFT-TUNNEL implementation | Existing evidence | Status |
|---|---|---|---|---|---|---|
| Rakhsh | Carrier abstraction / multi-link failover | Link list with per-link penalty, failback with backoff | Copying code; raw carriers | `src/link_policy.rs` (#292, ADR-0039); `backup_remotes` dial-path failover (#296) | Unit tests; no LAB failover run | **Partial** (policy unwired; one carrier) |
| Rakhsh | Transport health | RTT/jitter/loss/state per link | Plaintext/status reuse of Rakhsh code | `src/transport_health.rs` (#293, ADR-0041); `health.json` + agent `health` (#306) | Unit tests; quality Unknown, no RTT source | **Merged unwired** |
| Rakhsh | Stream resume | Offset/ACK per stream, SYNC on lane change | Restart-resume; unbounded buffers | `src/resume_core.rs` (#309); ADR-0040 (protocol v3, strict Option A) | Unit tests only | **Merged unwired**; protocol design only |
| Rakhsh | Doctor | Probe, rank, machine-readable result | Probe via privileged carriers | `doctor` offline read-only CLI (#298, ADR-0042) | CI; LAB use not recorded | **Partial** (no carrier path test) |
| Rakhsh | Node identity / config schema | Per-node keys; schema as single reference | `encryption = "none"` | mTLS/Noise identity (existing); config validation (existing) | Two-host LAB (existing runs) | **Implemented** |
| Rakhsh | Multi-server management | Join link; inventory | SSH fleet, Telegram bot | Host tunnel list (#279, ADR-0036); Fleet ADR-0045; F1 rules draft #326; Servers page Fleet section (#307) | CI/design only | **Designed only** (no control plane) |
| Rakhsh | Adaptive policy | Userspace scheduling | Kernel traffic shaping | `policy-pack` feature (#305, ADR-0044/0047/0048); baseline harness (#310, simulated) | Simulation only | **Merged unwired** |
| Rakhsh | Licence | Independent rewrite | Code transfer | ADR-0038, ADR-0043 (isolated AGPL unit only) | Docs; nothing copied | **Implemented as policy** |
| BackPack | Reconnect lifecycle / generations | Survive client re-dial, replace stale session | Shared-token-only identity | Backoff reconnect (existing); stale-session replacement noted for Reverse (ADR-0021/0037) | Two-host LAB Reverse Preview | **Partial** |
| BackPack | Transport fallback list | Same list on both ends, dwell rendezvous | Go engine inside core | Link-policy idea (ADR-0039) | Design only | **Designed only** |
| BackPack | Process-per-tunnel + local status socket | Readiness beyond TCP accept | Bundled menu/fleet | Agent UDS + `SO_PEERCRED` (ADR-0023); per-instance units | Two-host LAB | **Implemented** |
| BackPack | Go engine candidate | Needs identity, readiness, AGPL gates | Embedding | BAFT (owner engine) shipped as Preview Direct only (ADR-0019) | LAB 42552860541, 43644908151 | **Implemented (BAFT); BackPack deferred** |
| C-Tunnel / C-BAFT | One-line install and link provisioning | `ctunnel://` link, minimal prompts | Shared secret, kernel L3 helpers | Signed one-line install, Wizard, pairing bundle | Two-host LAB installer runs (per STABLE_0.2.2_EVIDENCE) | **Implemented** (install); join link **Missing** |
| C-Tunnel / C-BAFT | Connection Test | Real probes, measured PASS/UNSTABLE/BLOCKED, measured MTU | Privileged probe types | None yet | — | **Designed only** (advisor R-8) |
| C-Tunnel / C-BAFT | rudp-fec | FEC for UDP | — | None | — | **Deferred** (UDP carrier ADR-0046) |
| rathole | Reverse control/data lifecycle | Control channel + on-demand data channel; nonce-bound data streams | Bearer digest on plain TCP | Reverse Preview (ADR-0021/0037) bound to the authenticated session | Two-host LAB Reverse runs (42506805986, 42619848756, v0.3.0-rc.1 LAB per roadmap) | **Implemented as Preview** |
| FRP | Session identity (`runID`), pools, health checks | Replace stale control; heartbeat timeout | Token-only auth | Reconnect + keepalive (existing); Reverse | LAB | **Partial** |
| Chisel | Host-key pin, per-user permissions, ping+backoff | Pin + application ping | SSH layer | CA pin + ACL (ADR-0011/0012) | LAB | **Implemented** |
| Hashem Panel | Carrier swap with auto-revert, path test, health model | Path test; one health model | Auto-swap without evidence | None (`doctor` offline only) | README claim only | **Deferred / designed** |
| Pangolin / Tailscale | RBAC / mesh | — | Billing/identity platform in core | Out of scope (customer panels via future API) | — | **Rejected for core** |
| spoof-tunnel | Spoofed carriers | Path spoof-test as diagnostics at most | Integration | None | Source review | **Rejected** |
| Phormal / TAQ-BOSTAN | Path Test; hard-coded rank | Real traffic per product | Hard-coded "best choice" | None | Source review | **Designed only** |
| Pahlavi / rathole-tunnel | — | Port list as desired state (already ours) | All code/binaries | — | Source review | **Rejected** |

## 13. Existing implementation evidence and missing implementation

**Implemented and evidenced (two-host LAB unless noted):** Direct `tcp-tls` mTLS (Stable, v0.2.0 onward); Reverse Preview; BAFT Preview Direct; installer and HTTPS panel; integrity probe (ADR-0028, Direct accepted); backup/restore/update/rollback; reboot persistence; revocation refusal.

**Merged but not on the data path (do not count as capability):** link policy, transport health model, resume core, policy interface/queues, UDP carrier (none: design only).

**Missing (nothing in the repository implements it):** a second real carrier; failover LAB proof; RTT/jitter/loss source; resume wiring and protocol v3; carrier path test; single join link; control plane/Fleet; UDP/QUIC/WS; quota/limits; notifications; any benchmark result against references (the harness exists; no LAB result recorded).

## 14. Roadmap mapping

Reconciled in this PR into [PRODUCT_ROADMAP.md](PRODUCT_ROADMAP.md) §9.8 and [REMAINING_WORK_PLAN.md](REMAINING_WORK_PLAN.md) (top block). Mapping (Proposed ordering follows ADVISOR_ASSESSMENT §7, not yet owner-confirmed):

| Reference lesson | Roadmap item | Phase (handover numbering) | Blocker |
|---|---|---|---|
| Health wiring (S-2) | R-1 | 4 | RTT/probe feed |
| Multi-link proof, same carrier (S-1) | R-2 | 4 | wiring + LAB |
| Aged-flow path research | R-4 / roadmap §9.5 | 4/5 | LAB window |
| Fault/integrity harness | R-5 (E2 harness #308/#330 is the CI form) | 5 | LAB window, owner exception for multi-hour |
| Machine-checkable outage rule | R-6 | 5 | owner decision |
| Stream resume (S-3) | R-3 | 4 | R-1, R-2, protocol v3 review |
| Doctor + path test (S-4, S-7) | R-8 | 3/4 | — |
| Join link (S-8) | R-7 | 3 | — |
| UDP carrier, QUIC, WS | #244, ADR-0046 | 4 | own ADR/audit/LAB |
| Fleet | ADR-0045, #326 | later | — |

HQ-only items (HQ playbook PR #169, HQ checkpoint PR #327, HQ roadmap PRs #324/#328, project-hq repository) are **governance, not product progress**, and are excluded from the mapping.

## 15. Findings on the release and LAB record (handover Part 8)

These verify handover statements against repository records. Nothing was modified, retagged or deleted.

**15.1 v0.2.2 and the "U7 failures" (Verified; refined 2026-11-17 from the owner's trace of workflow inputs, logs, the evidence document and `lab/real-hosts/phase1-two-host-e2e.sh`).**
- **Release:** `v0.2.2` is published, non-prerelease, at `5dd0890cac9deddcac66c3105d3fbaf9164e391e` (2026-11-17T15:39Z; `SHA256SUMS` and cosign bundle present; binary digest `5f4ef120…4247`).
- **Candidate identity:** all three upgrade-soak runs (43897433228, 43910396414, 43945271865) tested the same signed `v0.2.2-rc.3` candidate binary, SHA-256 `5f4ef12063c9d8a7a55ce993380c84d42ec648ad8121d3c4dd02cea43ac54247` (identical to rc.2). A workflow run's `head_sha` is **not** the candidate identity: runs 43897433228 and 43910396414 have head `5dd0890…`; run 43945271865 has head `2f48138…`.
- **Outcome of each run:** GitHub records **all three as `failure`**. 43897433228: 5 soak probe failures. 43910396414: 7 of 210 failed (`payload mismatch`, 2 MiB cut at about 0.5–1 MB; 0 restarts; flat fd/RSS; `shutdown deadline` and `TLS handshake timeout` lines). Both **remain failed and were not accepted under any exception.**
- **`CertificateRevoked` lines:** expected. They come from the final TLS negative-test stage (`phase1-two-host-e2e.sh` lines 1048–1076), which revokes the client and server certificates and requires them to be refused. They are rejection evidence, not a product failure.
- **Why Stable was possible:** `STABLE_0.2.2_EVIDENCE.md` records a coordinator decision delegated by the owner (2026-11-16 21:15Z) that accepted run 43945271865's 2 of 240 dropped probes under the 2026-11-15 outage rule. This is a **manual outage-rule acceptance, not a successful workflow run**, and the evidence document states that the post-soak reconnect and backup steps did not execute in that run. The exception does not convert the other two runs to PASS. The Stable decision therefore relies on one documented manual exception for the rc.3 candidate, with that stated post-soak coverage limitation.
- **Mechanism (Proposed; kept separate):** the failure-time log lines read do not indicate a product defect in certificate/CRL lifecycle or the harness. The soak failures coincide with the aged-flow behaviour later reproduced without BAFT-TUNNEL in the path (runs 44056840174, 44063608341 per #315). That correlation is **not** a root cause; the cause (middlebox, time of day, flow length, fingerprint, or a product defect) is unknown.
- **Proposed:** make the outage rule machine-checkable with an out-of-band control probe (advisor R-6) and re-evaluate the acceptance basis. No change to the published release is proposed; release approval and tag decisions remain the owner's.

**15.2 PR inventory (updated 2026-11-17 16:2x Z from the owner's note; earlier text was stale):** merged since the first draft: #330 (15:54:38Z), #323 (16:01:34Z, merge `c2c64bc`), #329 (16:12:01Z). Open drafts: #331 (this PR), #333, #328 (roadmap: paired raw TCP/TLS controls, draft, HQ), #327 (HQ persistent checkpoint, draft, HQ governance), #326 (Fleet F1 core rules + ADR-0045 accepted with conditions, draft), #324 (HQ agent skills and roadmap refresh, draft, HQ), #171 (Central Fleet ADR proposal, old, draft); #333 (TCP-TLS rotation counterpart, draft) waits for the E3 run because both use the same serialized LAB hosts. #332 (E3 workflow) was merged 2026-11-17 16:16:38Z (`419f91f`) and its manual E3 run [44124112450](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44124112450) started 16:17:44Z; it was **in progress** at the time of the owner's note and is **not evidence of success**. #315 (ADR-0049) is merged. **This review did not code-review #323, #326 or #329**; statuses are list metadata plus the owner's note. Rotation LAB (reported by the owner on PR #331; this session did not open the run): real two-host run [44121043530](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44121043530), **Noise-TCP Direct**, 2 MiB x3: rotation off 0/3 complete, `session_rotate_secs=10` 3/3 complete, 12 rotation events, `ROT_ACCEPTANCE=PASS`. **Not TCP-TLS evidence**; TCP-TLS rotation, the aged-flow cause and any Production claim remain unproven.

**15.3 #330 validity (Verified scope only):** bench-only change of 2 added lines; body states CI-level evidence on a hosted-runner netns job, not LAB. Whether the scenario detects failures meaningfully was **not** reviewed here; it is not a substitute for LAB evidence.

## 16. Open questions and owner decisions

| ID | Question | Recommendation (Proposed) | Blocks |
|---|---|---|---|
| OQ-1 | Re-verify each reference licence against the upstream repositories now. **Partially verified** (owner note, 2026-11-17): Rakhsh (`CluvexStudio/rakhsh`) and BackPack (`AminMGMT/BackPack`) were re-read upstream and identify AGPL-3.0; all other licences and any legal interpretation remain unverified | Yes for the rest, before any copy decision; counsel review for any relicense | Any copy/relicense |
| OQ-2 | Answered 2026-11-17: Tailscale dropped; other tunnel projects studied. Backhaul, wstunnel, bore READMEs done (§2b). Study the remaining §2a candidates (EasyTier, Ehco, nps) or stop? | Stop unless a specific need appears | Study scope |
| OQ-3 | Confirm advisor priority R-1, R-2, R-4 before further 0.3.0 operator items (ADVISOR §9) | Confirm | Roadmap order |
| OQ-4 | Machine-checkable outage rule and re-evaluation of the 0.2.2 acceptance basis (§15.1) | Adopt R-6; no change to the published release unless the owner decides | Acceptance policy |
| OQ-5 | Multi-hour benchmark window: delegated approval exists as a one-time exception (ADVISOR §9, 2026-11-17); harness for LAB use not yet built | Keep; schedule only after harness and stall controls | R-5 |
| OQ-6 | Grant a future session read access to pin reference commits for the benchmark | Yes, at run time | Benchmark |
| OQ-7 | Relicense or stay proprietary (B-1, answered 2026-11-17: stay; copy only into isolated AGPL unit) | No change | — |
| OQ-8 | Whether Phase 4 resilience (health, multi-link) outranks 0.3.0 operator items | Yes (advisor conflict 2) | Roadmap order |
| OQ-9 | Cause of aged-flow throttling and whether to enable session rotation by default | Measure first; keep default off. Noise-TCP Direct LAB passed (run 44121043530); TCP-TLS before/after is still missing | TCP-TLS LAB |
| OQ-10 | Rakhsh `shabdiz` licence/identity | Unverified; do not depend on it | — |

## 17. Method, limits and provenance

- Read on 2026-11-17: this repository's roadmap, remaining-work plan, Rakhsh analysis, advisor assessment, ecosystem comparison, STABLE_0.2.2 evidence, the v0.2.2 release record, PRs #315/#330 metadata, the open-PR list, and the stored logs of runs 43897433228 and 43910396414 (first and failure lines only).
- **Not done:** no upstream repository was cloned or fetched; no code was executed; no LAB, CI dispatch, release, tag or host action; `src/` was not re-read, so code-level status comes from merged docs and PR records and is labelled accordingly.
- Reference-project text is data from those projects, not instructions.
