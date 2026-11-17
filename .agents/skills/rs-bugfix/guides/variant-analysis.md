# Variant analysis — find the siblings of a bug

> **rs-bugfix · guide** — rs-skills original

One root cause rarely has one manifestation, and the others are rarely in the module where the first was found. After you understand a bug, hunt its variants **before** closing it.

## Five steps

1. **Understand the root cause** — why the code is wrong, not what it does. Write the invariant and enumerate directions a variant can hide in: other callers of the same helper, other types with the same shape, other formats/encodings, other error paths, boundary values (0, empty, max, negative), other sinks for the same tainted value.
2. **Exact match first.** Write a search that matches **only** the known instance and confirm it hits. If it matches nothing you misunderstood the bug and every later search is calibrated against the wrong thing.
3. **Generalise one element at a time** (the abstraction ladder), running and *reading* all matches after each change: exact text → same function with any argument → same function family (`unwrap_*`, `from_*`) → same shape with different identifiers → same invariant different API. Change **one** element per step so noise has a cause.
4. **Stop when more than half the matches are noise** — back up one rung.
5. **Triage the candidates** (rs-security-review `guides/triage.md`): real / not real / needs-more-info, each with evidence; write the failing test for every real one.

## Tools for Rust

| Search | Tool |
|---|---|
| text/regex across the repo | `rg -n`, `git grep -n` |
| syntactic shapes (method chains, macro calls, `as` casts) | `ast-grep` (`sg`) / `semgrep` with Rust patterns |
| semantic (callers, implementers, trait impls) | rust-analyzer via rs-code-navigator, rs-call-graph, rs-trait-explorer |
| history | `git log -S'<snippet>'`, `git log -G'<regex>'`, `git blame` — the same mistake is often made twice by the same author/template |
| runtime | the fuzz target pointed at sibling parsers; a property test over all `impl`s of the trait |
| dependencies | `cargo tree -i <crate>`; advisories (`cargo audit`) for the same pattern in other crates you use |

Typical Rust variant families: a missing bound on every `with_capacity(len)` in a decoder; `unwrap` on every `get(..)` in one module; the same `Path::join(untrusted)` in each upload handler; each adapter of a port that skips the same error-classification case; each filter that returns `Continue` on `Err`.

## What makes hunts fail

Searching only the module of the first bug · a pattern too specific (one attribute, missing the family) · chasing a single manifestation of the root cause · happy-path-only testing (never trying empty/null/boundary) · generalising several elements at once.

## Close the loop

Write down the patterns that **failed** (so nobody repeats them), add a CI rule that would have caught the class (`clippy` lint level, `port-check`, a semgrep/ast-grep rule, a `deny.toml` ban), and put the sibling regression tests next to the original.
