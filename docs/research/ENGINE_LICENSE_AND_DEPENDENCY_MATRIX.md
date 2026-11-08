# Engine License and Dependency Matrix (research, docs only)

- Status: research report v1.0, 2026-11-08. This is an engineering reading of licence texts and manifests, **not legal advice**. Anything marked "owner/legal decision" needs a human decision before code is used.
- Baseline: BAFT-TUNNEL Rust is `LicenseRef-Proprietary` (`Cargo.toml`). ADR-0019 already fixes the model for copyleft engines: separate pinned binary, source link and exact tag published, no linking, no relicensing.
- Method: licence files and manifests read from shallow clones of the upstream repositories listed in `ENGINE_ECOSYSTEM_COMPARISON.md`. Transitive licence data was **not** produced by tooling (no `go-licenses`, `cargo-deny` run); entries for dependencies come from reading `go.mod` / `Cargo.toml` and known upstream licences, and are flagged "verify with tooling" in section 4.

## 1. Project licences

| Project | License (file) | Copyright holder text | Class | Can we embed or link its code in proprietary Rust? | Can we ship it as a separate binary alongside? |
|---|---|---|---|---|---|
| BackPack | AGPL-3.0 (`LICENSE`) + `NOTICE` additional terms under s.7 | Amin Mohammadi (AminMGMT), 2026 | Strong copyleft, network clause | **No** (would make the combination AGPL) | Yes, as a separate process, with obligations below |
| rathole | Apache-2.0 (`LICENSE`, `Cargo.toml`) | Yujia Qiao et al. | Permissive + patent grant | Yes, with NOTICE/attribution retained and modified-file notices | Yes |
| frp | Apache-2.0 | fatedier et al. | Permissive | Yes (attribution) | Yes |
| chisel | MIT | Jaime Pillora, 2024 | Permissive | Yes (keep notice) | Yes |
| spoof-tunnel | Apache-2.0 | ParsaKSH | Permissive | Technically yes, **not recommended** (see risk doc) | Not planned |
| TAQ-BOSTAN | Apache-2.0 | ParsaKSH, 2025 | Permissive | Scripts only; do not use as infrastructure | Not planned |
| Phormal | GPL-3.0 | Schmi7zz | Strong copyleft | No | Not planned (script orchestrator) |
| rathole-tunnel | GPL-3.0 | Musixal | Strong copyleft | No | No; contains committed binaries of unknown provenance |
| Pahlavi-tunnel | **No LICENSE file** | - | All rights reserved by default | **No**: no permission granted | No |
| M-R-R-X/ctunnel | unknown (repository not verified) | - | - | No | No |

## 2. BackPack obligations in detail

Source: `LICENSE` (AGPL-3.0 text) and `NOTICE`.

1. **AGPL-3.0 s.13 (network use).** If we run a *modified* BackPack for others over a network, we must offer those users the corresponding source of the modified version. Running an *unmodified* pinned build still triggers the general GPL duty to provide source on conveying the binary (s.6) whenever we distribute it (release artefact, image). Practical rule: ship the exact tag/commit source URL and a source archive with every release that contains the binary.
2. **NOTICE s.7(b) attribution.** If a modified version is conveyed, or run for others over a network, the line `Based on BackPack by Amin Mohammadi (AminMGMT) https://github.com/AminMGMT/BackPack` must stay visible and unaltered in our NOTICE file **and** in the README of the distribution. Adding our own notices beside it is expected.
3. **NOTICE s.7(e) trademark.** "BackPack", its name and logo are not licensed. A modified version must carry a different name and branding. Saying truthfully that a work is based on or compatible with BackPack is always allowed. In the panel the engine must be named so that it does not imply BackPack's endorsement; unmodified use may keep the name as a factual identifier, modified builds must be renamed.
4. **Third-party attribution inside NOTICE.** BackPack states that portions of the data plane derive from earlier AGPL/GPL work and that its SNI-spoofing carrier follows the technique of patterniha/SNI-Spoofing and the MIT-licensed Rust implementation by therealaleph. Those notices must be preserved with the binary. If we never enable or expose the SNI-spoof carrier, the notice still travels with the binary.
5. **Contributor consent.** Relicensing or combining under another licence needs consent of all contributors (same finding as ADR-0019 for BAFT). Not needed for the separate-process model.
6. **Modification policy.** Patches (for example a readiness change or a `/run` directory fix) make it a modified version: rename the build, publish the patch set and the source for it, and keep a patch series rather than a private fork.
7. **Security disclosure channel.** Upstream asks for disclosures to the address in `SECURITY.md`; our process for upstream bugs should use it rather than public issues.

Decision needed from the owner/legal (not decided here): accept the above obligations for BackPack as a separately packaged engine. Until recorded in an ADR, the manifest entry must not be added. Because the owner has stated BAFT and C-BAFT are their own code, a *separate* arrangement with the BackPack author (a commercial licence or an explicit exception) is also possible, but that is a business decision.

## 3. Other licence notes

| Topic | Note |
|---|---|
| Apache-2.0 reuse (rathole, frp) | Keep `LICENSE`, retain `NOTICE` content if present, mark changed files. Patent grant is a benefit. Rewriting the idea in our own Rust (preferred for rathole's role) creates no licence obligation, but copying structure/code does. |
| MIT (chisel) | Include copyright and permission notice in distributions containing chisel. |
| GPL-3.0 scripts (Phormal, rathole-tunnel) | Do not copy code. Ideas and observable behaviour are not covered, but we should write our own path-test logic from the described behaviour, not from their code. |
| Pahlavi (unlicensed) | Do not copy anything. The "port auto-sync" idea is trivial and also exists in other projects. |
| Hysteria2 / gost binaries used by Phormal and TAQ-BOSTAN | Downloaded from third-party releases at install time; each carries its own licence (Hysteria is MIT; gost is MIT per upstream, verify). Not relevant unless we ship them, which is not proposed. |
| Committed binaries (`rathole-tunnel/core/*.zip`) | SHA-256 prefixes of three zips were recorded for reference (`da96a275...`, `3dac37a4...`, `01525bc5...`). "modified" builds with no source: unusable. |

## 4. Dependency review

Counts and notable items from manifests. Per-dependency licences below are the upstream-published licences as known to the author of this report and are **to be confirmed with tooling** (`go-licenses report`, `cargo deny check licenses`, `govulncheck`, `cargo audit`) in the first implementation PR that actually pins an engine. Nothing here replaces that.

### 4.1 BackPack `go.mod` (go 1.26.9)

| Dependency | Role | Expected licence (verify) | Note |
|---|---|---|---|
| github.com/BurntSushi/toml | config | MIT | |
| github.com/cilium/ebpf | XDP spoof receiver | MIT | Only the optional spoof path needs it; still linked |
| github.com/flynn/noise | stealth, l3 | BSD-3-Clause | Noise implementation |
| github.com/gorilla/websocket | ws/wss | BSD-2-Clause | |
| github.com/klauspost/reedsolomon | FEC | MIT | |
| github.com/quic-go/quic-go | quic | MIT | |
| github.com/refraction-networking/utls | camouflage ClientHello | BSD-3-Clause | |
| github.com/shirou/gopsutil/v4 | metrics | BSD-3-Clause | |
| github.com/sirupsen/logrus | logging | MIT | |
| github.com/xtaci/kcp-go/v5 | kcp | MIT | |
| github.com/xtaci/smux | mux | MIT | |
| golang.zx2c4.com/wireguard (+ wintun indirect) | l3 pieces | MIT | wintun is Windows-only |
| golang.org/x/{crypto,net,sys,time,text} | | BSD-3-Clause | |
| github.com/tjfoc/gmsm (indirect) | via kcp-go | Apache-2.0 | |
| Others (brotli, purego, go-ole, compress, cpuid, errors, plan9stats, perfstat, sysconf, numcpus, wmi) | indirect | mostly MIT/BSD | verify |

Compiled-in surface that a managed engine does not need: web panel, Telegram, monitor, SSH fleet (`internal/node/ssh.go`), update, geo. All in the one binary, so they widen the audit scope even if never started.

### 4.2 rathole (Rust)

Direct dependencies: tokio, bytes, clap 3, toml 0.5, serde, anyhow, sha2, bincode 1, lazy_static, hex, rand 0.8, backoff, tracing, socket2 0.4, fdlimit, async-trait, snowstorm 0.4 (Noise), base64 0.13, notify 5.0 pre-release, atty, async-http-proxy, async-socks5, url, tokio-tungstenite 0.20, tokio-native-tls or tokio-rustls, p12, openssl (musl, vendored). Many are old majors; `atty` and `bincode` 1.x have public unmaintained advisories (from memory of RustSec; confirm with `cargo audit`). Default build uses native OpenSSL via native-tls, which on Ubuntu 22.04 means OpenSSL 3.0 system library. As a design reference this has no impact; if ever forked, a dependency refresh is a prerequisite.

### 4.3 frp, chisel

frp: ~40 direct modules including `k8s.io/client-go`, `apimachinery`, `go-oidc`, prometheus client, `quic-go`, `kcp-go`, `wireguard`, `netlink`, `songgao/water`; large transitive tree, Apache-2.0/MIT/BSD mix expected. chisel: small (`x/crypto/ssh`, gorilla/websocket, `jpillora/*` helpers, two SOCKS5 libraries); MIT/BSD expected.

## 5. Distribution risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| AGPL source duty not met in release | Licence breach | Release engine manifest carries `source_url` at exact commit (already in schema); release workflow attaches a source archive; NOTICE and README lines added; documented in `docs/RELEASE.md` when the first copyleft engine other than BAFT lands |
| Modified BackPack shipped under its name | Trademark breach | Rename modified builds; prefer unmodified pinned tag; carry patches as a series |
| Mixing copyleft code into proprietary Rust | Relicensing obligation | Separate process only; no FFI; no code copy from GPL/AGPL/unlicensed projects |
| Upstream disappears or force-pushes | Cannot rebuild | Mirror source archive of the pinned tag in our release assets with its SHA-256 |
| Unreviewed dependency CVE | Supply chain | `govulncheck`/`cargo audit`/`cargo deny` in CI for each pinned engine; reproducible build check (two builds, equal hash) |
| Third-party installer scripts | Host compromise | Never executed; binaries come only from our CI |
| Unlicensed/unprovenanced code (Pahlavi, committed zips) | No right to use | Excluded |
