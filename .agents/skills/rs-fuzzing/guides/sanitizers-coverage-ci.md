# Sanitizers, Miri, coverage and CI

> **rs-fuzzing · guide**

| Tool | Finds | Use |
|---|---|---|
| **AddressSanitizer** | out-of-bounds, use-after-free, leaks in `unsafe`/FFI | default under cargo-fuzz; for tests: `RUSTFLAGS="-Zsanitizer=address" cargo +nightly test --target <host-triple>` (nightly; macOS/Linux) |
| **ThreadSanitizer** | data races in `unsafe`/atomics code | `-Zsanitizer=thread` (needs `-Zbuild-std` for accurate results; heavy) |
| **MemorySanitizer** | uninitialised reads | Linux only, `-Zsanitizer=memory` with `-Zbuild-std` |
| **Miri** | undefined behaviour in `unsafe` (aliasing, alignment, uninit) — interpreter, slow, no FFI/most I/O | `cargo +nightly miri test` on the unsafe core (rs-unsafe `unsafe-miri-ci`) |
| `cargo geiger` | where `unsafe` is (yours and dependencies') | decides whether sanitizers matter at all |
| `loom` / `shuttle` | concurrency interleavings | rs-testing |
| `cargo llvm-cov` | coverage of unit/integration tests | `cargo llvm-cov --html`; gate on a floor, not 100% |

Sanitizers need nightly and rebuild the dependency graph with different flags → a **separate target dir** (`CARGO_TARGET_DIR=…/sanitizer-target`) and cleanup afterwards (rs-cleanup: each flag set is a full new set of artefacts).

## CI

- **PR job (minutes):** `cargo test` (replays corpus + regressions on stable) + a **smoke fuzz** per target: `cargo +nightly fuzz run <t> -- -max_total_time=60`. Fails on any crash.
- **Scheduled job (hours, nightly/weekly):** longer runs with a cached corpus; upload artifacts of crashes; open an issue (host-neutral: any tracker) with the minimised input.
- **Continuous fuzzing:** for open-source crates, OSS-Fuzz (Google) runs targets continuously and files reports; for private code use the same pattern on your CI schedule.
- Cache `fuzz/corpus` between runs; never commit large corpora.
- Pin the nightly (`rust-toolchain.toml` for the fuzz job only) so diagnostics and instrumentation stay stable (rs-release).
- Other engines exist (AFL++ via the `afl` crate, `honggfuzz`, LibAFL for custom fuzzers); start with cargo-fuzz and move only for a reason (multi-core scaling, custom mutators).
