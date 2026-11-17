# fuzz-harness-is-a-pure-function

> Put the harness in the library as a pure function; the fuzz target is one line
> **rs-fuzzing · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

If the harness lives only in `fuzz/fuzz_targets/*.rs` it runs only under nightly cargo-fuzz — nobody replays crashes in CI, regressions are forgotten and the logic cannot be unit-tested. As a pure function it runs everywhere.

## Do

```rust
// src/lib.rs
pub fn check_parse(data: &[u8]) { /* parse; assert properties; never I/O, time, threads, globals */ }
// fuzz/fuzz_targets/parse.rs
fuzz_target!(|data: &[u8]| { my_crate::check_parse(data); });
// tests/fuzz_corpus.rs  — replays fuzz/corpus/** and fuzz/regressions/** through check_parse on stable
```
See `templates/fuzz/` (compile-checked).

## Don't

Put logic in the fuzz target; use `rand`/`SystemTime`; read files or the network; `catch_unwind` the code you are testing.

## See Also

- [fuzz-crash-becomes-regression](fuzz-crash-becomes-regression.md)
