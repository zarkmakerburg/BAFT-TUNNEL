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

The product already ships `rustls 0.23` + `ring`, `tokio`, `libc`, `rand` and `zeroize`.
**Amended 2026-11-27 (A7): the recommended configuration adds nine new crate names but fifteen lock
packages**, taking the product lock from 189 to 204 packages. The six extra packages are second
versions of crates the product already had at another version, all required by `quinn-proto`
0.11.19's own manifest: `rand 0.10.4`, `rand_core 0.10.1`, `chacha20 0.10.2`, `cpufeatures 0.3.1`,
`thiserror 2.0.21` and `thiserror-impl 2.0.21`. They cannot be avoided by feature selection (`rand`
and `thiserror` are non-optional `quinn-proto` dependencies), and all six are MIT OR Apache-2.0, so
`deny.toml` is still unchanged. The nine new names are:
`quinn`, `quinn-proto`, `quinn-udp`, `lru-slab`, `rand_pcg`, `rustc-hash`, `tinyvec`, `tracing` and
`tracing-core`. (The scratch probe itself is not a product addition.)

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

Second versions of crates the product already had (amendment A7, 2026-11-27):

| Added package | Version | Licence | Already in the product as |
|---|---|---|---|
| `rand` | 0.10.4 | MIT OR Apache-2.0 | `rand 0.8.8` |
| `rand_core` | 0.10.1 | MIT OR Apache-2.0 | `rand_core 0.6.4` |
| `chacha20` | 0.10.2 | MIT OR Apache-2.0 | `chacha20 0.9.1` |
| `cpufeatures` | 0.3.1 | MIT OR Apache-2.0 | `cpufeatures 0.2.17` |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 | `thiserror 1.0.69` |
| `thiserror-impl` | 2.0.21 | MIT OR Apache-2.0 | `thiserror-impl 1.0.69` |

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
- **A7.** The lock delta is **fifteen packages, not nine**: nine new names plus the six second
  versions listed in section 2 (`rand`, `rand_core`, `chacha20`, `cpufeatures`, `thiserror`,
  `thiserror-impl`), required by `quinn-proto 0.11.19` and all MIT OR Apache-2.0. The earlier count
  deduplicated by name and missed them. No new licence, feature or policy decision follows from
  this amendment; `deny.toml` stays unchanged, and any future `quinn`/`quinn-proto` bump re-runs
  this review (A4).

## 9. What this review does not establish

- No independent security review of the design or the dependency tree has happened (gate (b)).
- ~~No fuzz target, no two-process loopback test (gate (c)).~~ **Gate (c) is delivered (2026-11-27):**
  a `udp-carrier`-gated prototype surface (`src/udp_carrier.rs`: bounded datagram frame and hello,
  QUIC endpoint/connection helpers), a fuzz target (`fuzz/fuzz_targets/udp_datagram.rs`), and a
  two-process loopback test that completes a real QUIC handshake and one datagram round-trip,
  enforced by the dedicated `udp-carrier` CI job and the `fuzz-smoke` `udp-datagram` job.
- ~~No per-tuple benchmark against the TCP carrier (gate (d)); the build numbers above are not
  performance.~~ **Gate (d) is delivered (2026-11-27):** `src/udp_carrier_bench.rs` (test-only,
  compiled only with `udp-carrier`) measures the prototype carrier against the product's TCP carrier
  layers. The tuple axis is (carrier, application payload size) — 64 B and 1024 B, both inside the
  1200-byte path-MTU assumption — because decision 10 leaves the five profiles' mapping onto
  datagram classes undecided, so a profile axis would be invented. The UDP arm is the real gate-(c)
  session (`crate::udp_carrier`: QUIC handshake, hello, negotiated RFC 9221 datagrams); the TCP arm
  is TLS 1.3 on `ring` with the pinned certificate plus a real `tokio-yamux` session built with
  `crate::tunnel::yamux_config(8)` and one stream. Each tuple: 20 warmup, 200 latency and 2000
  throughput sequential request/echo samples, measured with `std::time::Instant`; every sample must
  be a byte-exact echo. Release-mode local run on 2026-11-27 (4 vCPU, rustc 1.88.0, loopback):
  at 64 B, p50 246 µs TCP vs 464 µs UDP (1.89x), p99 2136 vs 3699 µs, echo throughput 145,231 vs
  89,792 B/s (0.62x); at 1024 B, p50 268 vs 401 µs (1.50x), p99 1108 vs 1995 µs, 1,992,747 vs
  1,484,536 B/s (0.75x). The dedicated `udp-carrier` CI job runs the harness in release, enforces
  only conservative sanity bounds (UDP p50 ≤ max(10x TCP p50, TCP p50 + 5 ms); UDP throughput
  ≥ 0.1x TCP) plus echo integrity, and uploads `udp-carrier-bench.json` (schema
  `baft-udp-carrier-bench/1`). Evidence label: `local-loopback-release`. It is **not** a per-tuple
  product measurement and not a performance claim; no benefit is shown — in this sequential harness
  UDP is 1.5-1.9x slower at the median. It does not measure head-of-line blocking under a competing
  bulk flow, loss, jitter, NAT rebinding or any real path; those remain gate (e) and later work.
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
