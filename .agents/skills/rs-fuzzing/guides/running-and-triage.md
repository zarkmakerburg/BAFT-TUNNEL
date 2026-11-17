# Running campaigns and triaging crashes

> **rs-fuzzing · guide** — flags per cargo-fuzz/libFuzzer; confirm with `cargo fuzz run --help` for the version you installed

## Run

```bash
cargo +nightly fuzz run frame_parse                      # sanitizer: address by default
cargo +nightly fuzz run --sanitizer none frame_parse     # safe Rust only (no unsafe in your code or deps): ~2x faster
cargo +nightly fuzz run frame_parse -- -max_total_time=300 -max_len=4096 -rss_limit_mb=2048 -dict=tokens.dict
cargo +nightly fuzz run frame_parse fuzz/artifacts/frame_parse/crash-<hash>      # reproduce one input
cargo +nightly fuzz run frame_parse fuzz/corpus/frame_parse -- -runs=0           # replay the corpus once
```
Read the output: `NEW` = coverage gain, `pulse` = progress, `INITED` = started, a stack trace = crash saved to `fuzz/artifacts/<target>/`. Timeouts (`-timeout=`) and OOM (`-rss_limit_mb`) are findings too (DoS).

## Triage a crash (one at a time)

1. **Reproduce** with the artifact; run without the fuzzer (`cargo test` replay) and in a debug build.
2. **Minimise**: `cargo +nightly fuzz tmin frame_parse <artifact>` — the smallest input that still fails.
3. **Classify**: panic on input (`sec-panic-on-input`), hang/time (`sec-resource-exhaustion`), OOM, stack overflow (`sec-recursion-depth-dos`), sanitizer report (UB — rs-unsafe), property violation (logic bug). Run the **triage** (`rs-security-review/guides/triage.md`): who can send this input?
4. **Regression file**: copy the minimised input into `fuzz/regressions/` (replayed by `tests/fuzz_corpus.rs` on stable) and write the focused unit test.
5. **Fix at the invariant**, not at the symptom, then hunt variants (`rs-bugfix/guides/variant-analysis.md`).
6. Keep going: a harness that finds one bug usually finds the next.

## Corpus hygiene

- Seeds: real samples, spec examples, minimised past crashes, smallest valid and invalid inputs.
- `cargo +nightly fuzz cmin frame_parse` minimises the corpus (same coverage, fewer files); commit the **small** seed set, keep the large evolving corpus outside git (CI cache/artefact store).
- Never put secrets or customer data in a corpus (rs-git `git-no-secrets-no-artefacts`).

## Coverage — did the fuzzer reach it?

`cargo +nightly fuzz coverage frame_parse` then render with `llvm-cov` (`cargo llvm-cov` for unit tests). Uncovered regions are: unreachable code, behind an obstacle (checksum, magic), or needing structure — adjust the harness/dictionary/seeds and re-run. Zero coverage after an hour on a branch you expected is information.
