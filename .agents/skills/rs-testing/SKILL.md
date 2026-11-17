---
name: rs-testing
description: "Use when writing, organising or reviewing Rust tests, or deciding how to test something. Triggers: test, tests, unit test, integration test, doctest, #[test], #[cfg(test)], should_panic, test fixture, mock, mocking, mockall, trait mock, property test, proptest, quickcheck, snapshot test, insta, fuzz, loom, concurrency test, async test, tokio::test, benchmark, criterion, test coverage, flaky test, how do I test this, is this testable, arrange act assert"
user-invocable: false
---

# Testing

> **Layer 4: Delivery** · how we know it is correct — now and after the next change

## Core Question

**How will we know this is correct — today, and after someone else changes it?**

Tests are the executable half of the Design Contract: every **Invariant** line
should have a test that would fail if it were violated.

---

## Thinking Prompt

1. **What kind of code is it?** Pure logic → unit + property tests. I/O boundary → integration tests with an in-memory adapter. Public API → doctests. Concurrency → loom / tokio tests. Performance claims → criterion.
2. **What would break silently?** Those are the first tests to write.
3. **Can it be tested without the real world?** If not, introduce a trait at the boundary (rs-traits) and mock it.
4. **Where does the test live?** Same file (`#[cfg(test)] mod tests`) for units; `tests/` for black-box; `benches/` for criterion.

---

## The pyramid, in Rust terms

| Level | Where | Tool | Rules |
|---|---|---|---|
| Doctests | `///` examples on public items | `cargo test --doc` | `test-doctest-examples` |
| Unit | `#[cfg(test)] mod tests { use super::*; }` in the same file | built-in | `test-cfg-test-module`, `test-use-super`, `test-descriptive-names`, `test-arrange-act-assert`, `test-should-panic` |
| Property | same module | `proptest` | `test-proptest-properties` |
| Snapshot | same module or `tests/` | `insta` | `test-snapshot-testing` |
| Integration | `tests/*.rs`, one file per feature | built-in | `test-integration-dir`, `test-fixture-raii` |
| Mocks | trait at the boundary | hand-written or `mockall` | `test-mock-traits`, `test-mockall-mocking` |
| Concurrency | same module | `loom`, `#[tokio::test]` | `test-loom-concurrency`, `test-tokio-async` |
| Benchmarks | `benches/` | `criterion` | `test-criterion-bench` |

## Decision guide

| You have… | Test with |
|---|---|
| a pure function with many inputs | property test (`proptest`) + a few named examples |
| a parser / serializer | round-trip property (`parse(print(x)) == x`) + snapshot of error messages |
| a type with invariants (Design Contract **Invariants**) | one unit test per invariant, named after it |
| an I/O adapter | integration test against the real thing behind a feature flag + unit tests through the trait with a fake |
| a state machine | table-driven test of every transition, including the illegal ones (`should_panic` or `Err`) |
| async code with timing | `#[tokio::test(start_paused = true)]` and `tokio::time::advance` — never `sleep` |
| lock-free / `unsafe` concurrency | `loom` model, plus Miri in CI (rs-unsafe `unsafe-miri-ci`) |
| a performance claim | criterion bench with `black_box`, committed baseline |

---

## Production checklist

- [ ] every **Invariant** in the file's Design Contract has a test
- [ ] tests are named `fn <unit>_<scenario>_<expected>()`, not `test1`
- [ ] no `sleep`, no real network, no shared mutable global state between tests
- [ ] fixtures clean up via RAII (`TempDir`, guard structs), not manual teardown
- [ ] `cargo test --all-targets --all-features` and `--no-default-features` both pass in CI (rs-release)
- [ ] doctests compile and run (`cargo test --doc`); use `no_run` not `ignore` for examples that need resources

## Anti-Patterns

| Anti-pattern | Why bad | Better |
|---|---|---|
| testing through `main` only | slow, coarse, flaky | lib + thin main (rs-structure), test the lib |
| mocking concrete structs via `cfg(test)` branches | production code knows about tests | trait at the boundary, fake in tests |
| `assert!(result.is_ok())` | hides the value | `let v = result.expect("…")` then assert on `v`, or `assert_eq!` |
| giant test with ten asserts | first failure hides the rest | one behaviour per test |
| `#[ignore]` to silence a flaky test | rot | fix the race (`loom`, paused time) |

## Trace Up ↑ / Trace Down ↓

| Field / need | See |
|---|---|
| what to assert | rs-design-contract (**Invariants**), rs-domain-model |
| making code testable | rs-traits (ports), rs-structure (lib/main split) |
| async test harness | rs-concurrency |
| gates in CI | rs-release |

<!-- rules-index:start -->
## Rules (16)

Depth rules under `rules/` — open the one the question needs, not all of them.

**test-**

- [`test-arrange-act-assert`](rules/test-arrange-act-assert.md) — Structure tests with clear Arrange, Act, Assert sections
- [`test-cfg-test-module`](rules/test-cfg-test-module.md) — Put unit tests in `#[cfg(test)] mod tests { }` within each module
- [`test-criterion-bench`](rules/test-criterion-bench.md) — Use `criterion` for benchmarking
- [`test-descriptive-names`](rules/test-descriptive-names.md) — Use descriptive test names that explain what is being tested
- [`test-doctest-examples`](rules/test-doctest-examples.md) — Keep documentation examples as executable doctests
- [`test-env-bound-integration`](rules/test-env-bound-integration.md) — Integration tests that need external tooling compile everywhere, run where the tooling exists — and the scaffold test is replaced, not kept
- [`test-fixture-raii`](rules/test-fixture-raii.md) — Use RAII pattern (Drop trait) for automatic test cleanup
- [`test-integration-dir`](rules/test-integration-dir.md) — Put integration tests in the `tests/` directory
- [`test-loom-concurrency`](rules/test-loom-concurrency.md) — Use `loom` to exhaustively test lock-free and concurrent code
- [`test-mock-traits`](rules/test-mock-traits.md) — Use traits for dependencies to enable mocking in tests
- [`test-mockall-mocking`](rules/test-mockall-mocking.md) — Use mockall for trait mocking
- [`test-proptest-properties`](rules/test-proptest-properties.md) — Use proptest for property-based testing
- [`test-should-panic`](rules/test-should-panic.md) — Use `#[should_panic]` to test that code panics as expected
- [`test-snapshot-testing`](rules/test-snapshot-testing.md) — Use snapshot testing (insta) for complex or serialized output
- [`test-tokio-async`](rules/test-tokio-async.md) — Use `#[tokio::test]` for async tests
- [`test-use-super`](rules/test-use-super.md) — Use `use super::*;` in test modules to access parent module items

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|---|---|
| Error paths to cover | rs-error-handling |
| Observability assertions (spans, fields) | rs-observability |
| Benchmarks and perf regressions | rs-performance |
| Contract tests, fakes and in-memory adapters | rs-ports-adapters |
| Containers/volumes started by tests must be removed | rs-cleanup |
| Deterministic simulation of network faults (turmoil, madsim) | rs-domain-distributed |
| Fuzzing parsers/decoders, mutation testing (are my tests good?) | rs-fuzzing |
| Fixing a bug test-first and hunting its variants | rs-bugfix |
