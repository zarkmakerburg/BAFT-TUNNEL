# fuzz-crash-becomes-regression

> Every crash input is minimised, committed under `fuzz/regressions/`, and replayed by `cargo test`
> **rs-fuzzing · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

A fuzzer finds a bug once; the fix can regress years later in a refactor. A committed minimised input plus a one-line replay test makes the fix permanent and costs nothing on stable.

## Do

1. `cargo +nightly fuzz tmin <target> <artifact>`.
2. `cp` the result to `fuzz/regressions/<short-name>.bin` (name it for the bug, not the hash) and add the focused unit test.
3. The corpus-replay test (`tests/fuzz_corpus.rs`) runs every file through the harness.
4. Commit input + fix together (`fix(parser): reject length prefix beyond MAX_PAYLOAD`), body explains the trigger.
5. Variant hunt (rs-bugfix).

## See Also

- [fuzz-harness-is-a-pure-function](fuzz-harness-is-a-pure-function.md)
