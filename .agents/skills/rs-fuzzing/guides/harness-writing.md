# Writing fuzz harnesses

> **rs-fuzzing · guide** — the example in `templates/fuzz/` compiles and passes on stable; the libFuzzer target needs `cargo-fuzz` (nightly)

## Setup (cargo-fuzz, the standard Cargo fuzzer; libFuzzer backend)

```bash
rustup toolchain install nightly          # cargo-fuzz instruments with nightly flags
cargo install cargo-fuzz                  # verify the version with `cargo fuzz --version`
cargo fuzz init                           # creates fuzz/ with Cargo.toml and fuzz_targets/
cargo fuzz add frame_parse                # another target
```
Fuzz needs a **library crate** surface: split `main.rs` into `lib.rs` + thin `main.rs` (rs-structure). Keep the `fuzz/` crate out of the main workspace members or add `[workspace]` deliberately; commit `fuzz/Cargo.toml`, targets, seeds and `regressions/`; ignore `fuzz/artifacts` and large corpora.

## The harness pattern

```rust
// src/lib.rs — the pure harness (see templates/fuzz/src/lib.rs)
pub fn check_parse(data: &[u8]) {
    if let Ok((frame, used)) = parse(data) {
        assert!(used <= data.len());
        let again = encode(&frame).expect("a parsed frame must re-encode");
        assert_eq!(&again[..], &data[..used], "round trip changed the bytes");   // property, not just "no crash"
    }
}
```
```rust
// fuzz/fuzz_targets/frame_parse.rs — one line
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| { fuzz_example::check_parse(data); });
```

## Structure-aware input

Raw bytes rarely get past syntax. Derive `Arbitrary` on the input types (behind a Cargo feature so the library stays light) and fuzz values:

```rust
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Frame { pub kind: u8, pub payload: Vec<u8> }

fuzz_target!(|frame: fuzz_example::Frame| { fuzz_example::check_roundtrip(&frame); });
```
`cargo fuzz fmt frame_parse <artifact>` prints the generated value for a crash. For sequences (state machines) fuzz a `Vec<Op>` and replay the ops against a model.

## Oracles beyond "doesn't crash"

| Oracle | Example |
|---|---|
| round trip | `decode(encode(x)) == x`; `encode(decode(bytes)) == bytes` for canonical encodings |
| differential | fast path vs obviously-correct slow path; your adapter vs the in-memory fake (rs-ports-adapters) |
| invariant after each step | totals match, no duplicate ids, sorted order, tenant isolation |
| resource budget | time/alloc counters stay below a bound (guards against hangs and bombs) |
| no leak | masked/redacted output never contains the secret (for filters) |

## Avoiding false crashes and stalls

- **No randomness, time, threads, global mutable state** in the harness — non-determinism makes crashes unreproducible and corpora noisy.
- Reject over-large inputs early (`-max_len`), and cap work inside the code under test (`MAX_PAYLOAD`); a harness that legitimately takes seconds starves the fuzzer.
- Initialise expensive state once (`std::sync::LazyLock`), not per input.
- Don't let the harness hide panics: avoid `catch_unwind` around the code you are testing.
- Fuzz related APIs **together** when state flows between them, separately when failures would mask each other.

## Obstacles and dictionaries

| Obstacle | Fix |
|---|---|
| checksum/MAC/signature check rejects every mutated input | in fuzz builds `#[cfg(fuzzing)]` (set by cargo-fuzz) skip or recompute the check — **only in the harness path**, never in release code |
| magic bytes / keywords / tokens (`"jsonrpc"`, `"tools/call"`, `Bearer`) | a dictionary file: `-dict=tokens.dict` (`kw1="jsonrpc"` lines) extracted from the spec/source |
| time- or randomness-seeded behaviour | inject a fixed clock/seed under `cfg(fuzzing)` |
| deep state needed before the interesting code | structure-aware input that builds valid prefixes, or a seed corpus of valid sessions |
