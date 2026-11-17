# Mutation testing with cargo-mutants

> **rs-fuzzing · guide** (also linked from rs-testing). Tool: `cargo-mutants` (crates.io, sourcefrog). Check `cargo mutants --help` for the flags of the version you install.

Coverage tells you code *ran*; mutation testing tells you whether tests would *notice* a bug. The tool makes small changes (`+` → `-`, `return x` → `return Default`, `if` → `true`, remove a function body) and re-runs your tests. **Caught** = a test failed (good); **missed/survived** = no test noticed (a gap or an equivalent mutant); **unviable** = didn't compile; **timeout** = counts as caught if the test hangs.

## Run

```bash
cargo install cargo-mutants
cargo mutants --list                          # what would be mutated (no run)
cargo mutants                                 # full campaign — slow: rebuilds and re-tests per mutant
cargo mutants --in-diff changes.diff          # only mutate lines changed in a diff (git diff origin/main > changes.diff) — use in PR CI
cargo mutants --file src/policy/combine.rs    # one file
cargo mutants -j 4                            # parallel jobs (each builds in its own temp copy: disk + RAM!)
cat mutants.out/missed.txt                    # the survivors (also caught.txt, timeout.txt, unviable.txt)
```
Config in `.cargo/mutants.toml` (`exclude_re`, `exclude_globs`, timeouts); `#[mutants::skip]` (crate `mutants`) marks functions where mutation is meaningless (logging, `Display`).

## Read the survivors

For each survivor decide:

| Survivor means | Action |
|---|---|
| a real behaviour no test asserts | **write the test** that fails on the mutant (assert the value, the error, the boundary) |
| equivalent mutant — behaviour unchanged (`i < n` vs `i != n` over a loop that always ends) | mark skip with a comment; don't chase |
| code that is dead or unreachable | delete it (rs-anti-pattern) |
| a bug the mutation exposed (the "wrong" version is actually correct, or vice versa) | **real finding** — investigate (rs-bugfix) |
| security-relevant (`deny` path, `validate`, permission check) | highest priority: a missed mutant on an authorisation branch is a missing test for the thing that matters (rs-domain-policy decision tables) |

Severity heuristic: survivors in authentication/authorisation, parsing, money, and error-to-response mapping first; logging and formatting last.

## Make it practical

- Run on the **changed code in PRs** (`--in-diff`) and the whole crate on a schedule; gate on "no new survivors in changed security-relevant functions", not on a global percentage.
- Speed: fast test suite first (`cargo nextest`), `--baseline skip` when the tree is known-green, exclude generated code and wasm-only/target-specific modules, split by file across CI jobs.
- **Disk and time**: every job copies the tree and builds a target → large. Use `-j` conservatively, a scratch `CARGO_TARGET_DIR`/tmp on a big volume, and delete `mutants.out/` and temp trees afterwards (rs-cleanup).
- Pair with property tests (`proptest`) and fuzzing: survivors often disappear once a property asserts the invariant.
