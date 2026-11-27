# TP-U dependency and licence review — QUIC/datagram stack (ADR-0046, gate 1)

**Status:** Accepted 2026-11-27 as TP-U's dependency/licence-review evidence (gate (a) of
[ADR-0046](ADR-0046-udp-datagram-carrier-design.md)). **Verdict: proceed with Option A under
amendments A1-A6.** This is evidence about crates, not an implementation: no dependency, feature,
source file, version or capability changes with it.
**Date:** 2026-11-27
**Related:** [ADR-0046](ADR-0046-udp-datagram-carrier-design.md) (accepted 2026-11-27),
[ADR-0031](ADR-0031-adaptive-traffic-policy-pack.md) decision 11, [ADR-0030](ADR-0030-read-only-firewall-visibility.md),
[ADR-0044](ADR-0044-engine-agnostic-policy-interface.md), [deny.toml](../deny.toml),
issue [#244](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/244).
**Evidence level:** local toolchain + published crate data + the project's own policy files. No
independent security review, no fuzz run, no benchmark and no LAB. Those gates remain open.

## 1. What was reviewed, and how

ADR-0046 selects QUIC via `quinn` + `rustls` behind a non-default feature. This review resolves the
real stack at the versions a Rust 1.88 / edition-2024 project gets today, then runs the repository's
own gates against it.

- Toolchain: `rustc 1.88.0 (6b00bc388 2025-06-23)`, `cargo 1.88.0`.
- Scratch manifests only, in `/tmp` (never added to the product `Cargo.toml`): edition 2024,
  `rust-version = "1.88"`, so cargo's MSRV-aware resolver (resolver 3) picks 1.88-compatible
  versions. Resolution reported "Locking … to latest Rust 1.88 compatible versions".
- `cargo-audit 0.22.1` and `cargo-deny 0.18.4` — the exact versions the `phase1-rust` job installs
  (`.github/workflows/phase1-rust.yml`, lines 26-33) — run against the scratch lockfile.
- `cargo deny check licenses sources` with **this repository's `deny.toml` unchanged**.
- Release build wall-clock and binary size of one minimal program that builds a `rustls`
  `ServerConfig` and a `quinn` endpoint, compared with an empty release binary on the same VM. One
  run each, no cache; treat the numbers as this VM's order of magnitude, not a benchmark.
- Advisory DB: RustSec, 1568 advisories, commit `c9f6c91a44dbad9d71742329e9df061279f9c4be`, fetched
  2026-11-27.

## 2. Resolved versions

| Crate | Version | Licence (crate metadata) | `rust-version` |
|---|---|---|---|
| `quinn` | 0.11.12 | MIT OR Apache-2.0 | 1.85 |
| `quinn-proto` | 0.11.19 | MIT OR Apache-2.0 | 1.85 |
| `quinn-udp` | 0.5.16 | MIT OR Apache-2.0 | 1.85 |
| `rustls` | 0.23.46 | Apache-2.0 OR ISC OR MIT | 1.71 |
| `ring` | 0.17.14 | Apache-2.0 AND ISC | 1.66 |
| `rustls-pki-types` | 1.16.0 | MIT OR Apache-2.0 | 1.60 |
| `rustls-webpki` | 0.103.17 | ISC | 1.71 |
| `tokio` | 1.54.1 | MIT | 1.85 |

The product already ships `rustls 0.23` + `ring`, `tokio`, `libc`, `rand` and `zeroize`, so the
**recommended configuration adds only nine crates** to the 186-package product lock:
`quinn`, `quinn-proto`, `quinn-udp`, `lru-slab`, `rand_pcg`, `rustc-hash`, `tinyvec`, `tracing` and
`tracing-core`. (The scratch probe itself is the tenth package in its metadata, not a product
addition.)

| Added crate | Version | Licence | `rust-version` |
|---|---|---|---|
| `quinn` | 0.11.12 | MIT OR Apache-2.0 | 1.85 |
| `quinn-proto` | 0.11.19 | MIT OR Apache-2.0 | 1.85 |
| `quinn-udp` | 0.5.16 | MIT OR Apache-2.0 | 1.85 |
| `lru-slab` | 0.1.3 | MIT OR Apache-2.0 OR Zlib | — |
| `rand_pcg` | 0.10.3 | MIT OR Apache-2.0 | 1.85 |
| `rustc-hash` | 2.1.3 | Apache-2.0 OR MIT | 1.77 |
| `tinyvec` | 1.13.3 | Zlib OR Apache-2.0 OR MIT | — |
| `tracing` | 0.1.44 | MIT | 1.65 |
| `tracing-core` | 0.1.36 | MIT | 1.65 |

The highest `rust-version` in the closure is 1.85 (`quinn*`, `tokio`, `rand_pcg`), below the
repository's declared 1.88 — no MSRV bump is needed.

## 3. Feature findings: two default-feature traps

The two problems found are **not** in the QUIC code; they come from features that are on by default
or one flag away. Both are avoidable without touching `deny.toml`.

1. **`platform-verifier` is a default `quinn` feature and fails the licence policy.**
   With `quinn = "0.11"` (defaults on), the closure includes `rustls-platform-verifier 0.7.1` →
   `webpki-root-certs 1.0.12`, licensed **CDLA-Permissive-2.0**. `cargo deny check licenses sources`
   with this repo's `deny.toml` rejects it:
   `error[rejected]: failed to satisfy license requirements` at
   `webpki-root-certs … license = "CDLA-Permissive-2.0"` (99 packages scanned, `licenses FAILED,
   sources ok`). It is also the wrong model for this product: BAFT-TUNNEL verifies a pinned peer
   identity, it does not consume the public root store.
2. **`bloom` adds a Zlib-only crate.** Enabling `bloom` pulls `foldhash 0.2.0`, whose sole licence is
   **Zlib**; the current allow-list has no `Zlib` entry, so `cargo deny` fails (74 packages,
   `licenses FAILED`). `Zlib` appearing inside an OR expression elsewhere (`tinyvec`, `lru-slab`) is
   fine — a permitted alternative satisfies it — but a Zlib-only crate is not.

**Recommended feature set** (resolved; 69 crate dependencies including the probe, `licenses ok, sources ok`):

```toml
quinn = { version = "0.11.12", default-features = false,
          features = ["runtime-tokio", "rustls-ring", "log"] }
rustls = { version = "0.23", default-features = false, features = ["ring", "std"] }
```

Enabled features on `quinn`: `log`, `ring`, `runtime-tokio`, `rustls-ring`; on `quinn-proto`: `log`,
`ring`, `rustls-ring`; on `rustls`: `ring`, `std`. That is TLS 1.3 only — correct for QUIC — with no
`aws_lc_rs`, no `tls12`, no `platform-verifier` and no `bloom`. It introduces no second crypto
backend: the product already builds `ring` through `tokio-rustls`.

## 4. Advisory status

`cargo audit` against the recommended lockfile: **0 vulnerabilities, 0 warnings**, 69 crate
dependencies scanned against 1568 advisories. The `aws-lc-rs` variant (below) also audits clean
(76 packages). This is a snapshot at the DB commit above; it says nothing about future advisories and
is not a substitute for the independent review in gate (b).

## 5. Crypto backend: `ring` vs `aws-lc-rs`

| | `ring` (recommended) | `aws-lc-rs` |
|---|---|---|
| Resolved | `ring 0.17.14` | `aws-lc-rs 1.19.0`, `aws-lc-sys 0.47.0` |
| Licence | Apache-2.0 AND ISC — allowed | ISC/Apache-2.0/MIT/BSD-3-Clause expression — allowed |
| Advisory status | clean | clean |
| `deny.toml` | passes unchanged | passes unchanged |
| Cost | already in the product; ~56 s clean dependency build; 2.68 MB unstripped / 2.17 MB stripped for the probe | another backend (~+1 MB of Rust plus a C library); 99 s clean dependency build; 5.28 MB unstripped / 4.62 MB stripped; needs a C toolchain at build time |
| Fit | same provider the product already uses | FIPS-capable, but duplicates the crypto stack and doubles the audit surface |

The amendment keeps `ring`. `aws-lc-rs` is not rejected on policy grounds — it passes — but it adds a
second crypto implementation, a C build dependency and roughly +2.6 MB unstripped (+2.45 MB stripped)
to the probe for no requirement in ADR-0046. Revisit only if a FIPS requirement appears.

## 6. Build and size cost (probe, this VM)

| Build | Clean build | Unstripped | Stripped |
|---|---|---|---|
| Empty release binary (baseline) | 1 s | 415,624 B | — |
| `quinn` + `rustls` + `ring`, server-config + endpoint program | ~60 s (deps ~56 s + probe ~4 s) | 2,678,248 B | 2,168,432 B |
| same program, `aws-lc-rs` provider | ~111 s (99 s deps + 12 s) | 5,277,792 B | 4,621,936 B |

Both probes include `rcgen` for the self-signed certificate; the baseline does not, so the delta is
an upper bound on the stack's cost. In product terms: one extra static crypto/QUIC layer of roughly
2.3 MB unstripped on top of the application binary, and roughly one extra minute of clean dependency
build. Nothing here is a runtime or throughput measurement.

## 7. `deny.toml` verdict

- **Recommended set: no policy change.** `cargo deny check licenses sources` passes with the
  repository's `deny.toml` exactly as committed. `sources` passes (crates.io only); `bans` is not part
  of the CI command and no wildcard or git source is introduced. The only output is
  `warning[license-not-encountered]` for allowances this closure does not use (`BSD-1-Clause`,
  `BSD-2-Clause`, …), which is pre-existing behaviour and not a failure.
- **If `platform-verifier` is ever enabled**, `CDLA-Permissive-2.0` would have to be added to
  `[licenses].allow`, with a recorded reason. The review recommends not doing that: the product does
  not use the public root store.
- **If `bloom` is ever enabled**, `Zlib` would have to be added to `[licenses].allow`. Also not
  recommended before an explicit decision: `bloom` is an optimisation, and the Zlib-only
  `foldhash` dependency is the only thing that would make the entry necessary.
- No `[bans]` or `[sources]` change is required by either variant.

## 8. Verdict

**Proceed with Option A (QUIC), with these amendments to the implementation:**

- **A1.** `quinn` is declared with `default-features = false` and exactly
  `["runtime-tokio", "rustls-ring", "log"]`. `platform-verifier` is never enabled; the carrier uses
  the product's pinned-identity verifier, not the public root store.
- **A2.** `rustls` stays `default-features = false` + `["ring", "std"]`; no `aws-lc-rs`/FIPS backend
  and no `tls12` in the QUIC path.
- **A3.** `bloom` stays off; enabling it requires adding `Zlib` to `deny.toml` with a recorded
  decision.
- **A4.** Pin the reviewed versions (`quinn 0.11.12`, `rustls 0.23.46`, `ring 0.17.14`) through the
  product lockfile, and re-run this review whenever `quinn`/`quinn-proto`/`rustls` takes a semver
  minor or the lockfile repins them. A dependency update is a new gate-(a) event, not a routine
  bump.
- **A5.** `deny.toml` is unchanged for this set; any later feature that needs `CDLA-Permissive-2.0`
  or `Zlib` is a policy decision recorded in the PR that enables it.
- **A6.** The carrier stays behind the non-default `udp-carrier` feature and no operator surface
  (config key, CLI, panel, API) is added; this review authorises a build-time dependency only.

## 9. What this review does not establish

- No independent security review of the design or the dependency tree has happened (gate (b)).
- No fuzz target, no two-process loopback test (gate (c)).
- No per-tuple benchmark against the TCP carrier (gate (d)); the build numbers above are not
  performance.
- No two-host LAB run (gate (e)); whether UDP works on the Iran-to-abroad path remains Unknown.
- The advisory snapshot is point-in-time; `cargo audit` clean is not a guarantee.
- The probe is not the product: no workspace lint/profile interaction, no `cargo deny check bans`,
  no cross-compilation and no packaging were exercised.

## 10. Reproduction

```console
mkdir -p /tmp/quic-review/src && cd /tmp/quic-review
# Cargo.toml: edition 2024, rust-version 1.88, dependencies as in section 3
cargo generate-lockfile
cargo audit --file Cargo.lock                       # 0 vulnerabilities, 0 warnings
cargo deny --manifest-path Cargo.toml check --config <repo>/deny.toml licenses sources
cargo build --release                               # size/time of the probe
```
