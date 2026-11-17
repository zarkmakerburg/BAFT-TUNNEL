---
name: rs-fuzzing
description: "Use to find bugs tests and review miss by feeding a program machine-generated inputs — cargo-fuzz/libFuzzer harnesses, structure-aware fuzzing with arbitrary, sanitizers, coverage, corpus and dictionaries, fuzzing obstacles, continuous fuzzing in CI, and mutation testing with cargo-mutants. Triggers: fuzz, fuzzing, cargo-fuzz, cargo fuzz, libFuzzer, fuzz_target, arbitrary, Arbitrary derive, corpus, crash artifact, minimize, sanitizer, AddressSanitizer, ThreadSanitizer, MemorySanitizer, Miri, coverage, llvm-cov, cargo-llvm-cov, dictionary, checksum, OSS-Fuzz, AFL, honggfuzz, property-based vs fuzz, parser robustness, mutation testing, cargo-mutants, surviving mutant, mutants.out, test quality, are my tests good"
user-invocable: false
---

# Fuzzing and Mutation Testing — adversarial inputs and adversarial tests

> **Layer 4: Delivery** · the testing phase's answer to "what did we not think of?" and "would our tests notice a bug?"
>
> Depth: `guides/harness-writing.md` · `guides/running-and-triage.md` · `guides/sanitizers-coverage-ci.md` · `guides/mutation-testing.md` · compile-checked starting point: `templates/fuzz/` (library + harness + corpus-replay test; the libFuzzer target itself needs nightly)

## Core Question

**What input — or what code change — would make this program misbehave, and would anything in our pipeline notice?**

Review finds what a person thinks of; fuzzing finds what nobody thought of (panics, hangs, OOM, logic divergences, UB behind `unsafe`); mutation testing finds what the tests would not notice. Use all three on code that parses, decodes, filters or authorises.

## When to fuzz what

| Target | Why | Oracle (what counts as a failure) |
|---|---|---|
| parsers/decoders (JSON-RPC/MCP frames, SSE, JWT/JWS, config/GCL, protocol messages, archive/file formats) | untrusted bytes; classic DoS/panic/OOM source | no panic, no hang, bounded memory; `encode(parse(x)) == x` |
| deserialisers with custom `Deserialize`/visitors | recursion and size bombs | depth/size limits hold (rs-security-review `sec-recursion-depth-dos`) |
| matching engines (regex wrappers, glob, pattern/redaction/masking rules) | pathological inputs, quadratic time | time/size budget, no panic, no leak of masked data |
| `unsafe` code and FFI wrappers | memory safety | ASan/Miri-clean; no UB |
| two implementations of one thing (your fast path vs the simple one; your adapter vs the in-memory fake) | **differential** fuzzing finds semantic bugs | outputs equal (rs-ports-adapters contract suite as oracle!) |
| state machines (saga, protocol handshakes) | sequences of operations | invariants hold after every step (model-based test with `arbitrary` op lists) |

Not worth fuzzing: pure glue, code that is only I/O, anything whose only oracle is "doesn't crash" and which cannot crash.

## The loop

```
1 pick target + oracle      2 write the harness (pure fn in the library: `check_*`; fuzz target = one line)
3 seed the corpus (real samples, spec examples, past bug inputs)     4 run (minutes → hours, with sanitizer)
5 triage crashes (reproduce → minimise → root cause → regression test in fuzz/regressions/ → fix)
6 check coverage — what did the fuzzer never reach? (dictionary / harness change / obstacle)
7 run short smoke fuzzing in CI; long runs scheduled or continuous (OSS-Fuzz)
```

## Rules

1. **Harness = a pure function in the library** (`check_parse(data: &[u8])`), the fuzz target one line that calls it. Then the same function runs in unit tests, property tests and corpus replay on **stable** (`templates/fuzz/tests/fuzz_corpus.rs`).
2. **Deterministic, bounded, no I/O or global state** in the harness; cap sizes (`-max_len`) and memory (`-rss_limit_mb`); never `unwrap` on `Err` that bad input legitimately produces — return, don't panic.
3. **Assert properties, not just absence of crash**: round trips, idempotence, invariants (`used <= len`), equivalence with a reference.
4. **Structure-aware input** with `arbitrary::Arbitrary` once raw bytes get stuck on syntax checks.
5. **Every crash becomes a regression file** replayed by `cargo test` forever.
6. **Sanitizers when there is `unsafe`**; without `unsafe` in your code or deps, run `--sanitizer none` for ~2× speed (`cargo geiger` tells you).
7. **Measure coverage** of the fuzz corpus; uncovered code is either unreachable, behind an obstacle, or needs a better harness.
8. **Mind the disk**: corpora, artifacts, `target/` for the nightly toolchain and mutation-testing temp trees grow fast — budget and clean (rs-cleanup).

## Trace

| Direction | Go to |
|---|---|
| ↑ Up | rs-security-review (what to fuzz: panic/DoS/recursion/exhaustion classes), rs-domain-gateway (filters parse hostile traffic), rs-domain-auth (token parsing) |
| ↓ Down | rs-testing (property tests with `proptest`, loom), rs-unsafe (Miri, sanitizers), rs-type-driven (make invalid states unrepresentable so there is less to fuzz) |
| → Delivery | rs-bugfix (crash → regression test → fix → variants), rs-release (CI smoke job), rs-cleanup (corpus/artifact/target budgets) |

<!-- rules-index:start -->
## Rules (3)

Depth rules under `rules/` — open the one the question needs, not all of them.

**fuzz-**

- [`fuzz-assert-properties`](rules/fuzz-assert-properties.md) — Fuzz for properties (round trip, invariants, equivalence), not only for crashes
- [`fuzz-crash-becomes-regression`](rules/fuzz-crash-becomes-regression.md) — Every crash input is minimised, committed under `fuzz/regressions/`, and replayed by `cargo test`
- [`fuzz-harness-is-a-pure-function`](rules/fuzz-harness-is-a-pure-function.md) — Put the harness in the library as a pure function; the fuzz target is one line

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|---|---|
| Unit, property, snapshot, concurrency tests | rs-testing |
| Hunting by review instead | rs-security-review |
| Turning a crash into a fix and its variants | rs-bugfix |
| Disk used by corpora and mutation trees | rs-cleanup |
