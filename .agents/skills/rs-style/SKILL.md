---
name: rs-style
description: "Use when asking about Rust code style or best practices. Keywords: naming, formatting, comment, clippy, rustfmt, lint, code style, best practice, P.NAM, G.FMT, code review, naming convention, variable naming, function naming, type naming"
user-invocable: false
---

# Rust Coding Guidelines (50 Core Rules)

## Naming (Rust-Specific)

| Rule | Guideline |
|------|-----------|
| No `get_` prefix | `fn name()` not `fn get_name()` |
| Iterator convention | `iter()` / `iter_mut()` / `into_iter()` |
| Conversion naming | `as_` (cheap &), `to_` (expensive), `into_` (ownership) |
| Static var prefix | `G_CONFIG` for `static`, no prefix for `const` |

## Data Types

| Rule | Guideline |
|------|-----------|
| Use newtypes | `struct Email(String)` for domain semantics |
| Prefer slice patterns | `if let [first, .., last] = slice` |
| Pre-allocate | `Vec::with_capacity()`, `String::with_capacity()` |
| Avoid Vec abuse | Use arrays for fixed sizes |

## Strings

| Rule | Guideline |
|------|-----------|
| Prefer bytes | `s.bytes()` over `s.chars()` when ASCII |
| Use `Cow<str>` | When might modify borrowed data |
| Use `format!` | Over string concatenation with `+` |
| Avoid nested iteration | `contains()` on string is O(n*m) |

## Error Handling

| Rule | Guideline |
|------|-----------|
| Use `?` propagation | Not `try!()` macro |
| `expect()` over `unwrap()` | When value guaranteed |
| Assertions for invariants | `assert!` at function entry |

## Memory

| Rule | Guideline |
|------|-----------|
| Meaningful lifetimes | `'src`, `'ctx` not just `'a` |
| `try_borrow()` for RefCell | Avoid panic |
| Shadowing for transformation | `let x = x.parse()?` |

## Concurrency

| Rule | Guideline |
|------|-----------|
| Identify lock ordering | Prevent deadlocks |
| Atomics for primitives | Not Mutex for bool/usize |
| Choose memory order carefully | Relaxed/Acquire/Release/SeqCst |

## Async

| Rule | Guideline |
|------|-----------|
| Sync for CPU-bound | Async is for I/O |
| Don't hold locks across await | Use scoped guards |

## Macros

| Rule | Guideline |
|------|-----------|
| Avoid unless necessary | Prefer functions/generics |
| Follow Rust syntax | Macro input should look like Rust |

## Deprecated → Better

| Deprecated | Better | Since |
|------------|--------|-------|
| `lazy_static!` | `std::sync::OnceLock` | 1.70 |
| `once_cell::Lazy` | `std::sync::LazyLock` | 1.80 |
| `std::sync::mpsc` | `crossbeam::channel` | - |
| `std::sync::Mutex` | `parking_lot::Mutex` | - |
| `failure`/`error-chain` | `thiserror`/`anyhow` | - |
| `try!()` | `?` operator | 2018 |

## Quick Reference

```
Naming: snake_case (fn/var), CamelCase (type), SCREAMING_CASE (const)
Files:  nouns or stage verbs per feature directory; never utils/helpers/common (rs-structure)
Format: rustfmt (just use it)
Docs: /// for public items, //! for module docs
Lint: #![warn(clippy::all)]
```

Claude knows Rust conventions well. These are the non-obvious Rust-specific rules.

---

## Related patterns

| Problem | Pattern / idiom | Page (rs-patterns) |
|---|---|---|
| Object creation | Constructor (`new`) + `Default` | `../rs-patterns/idioms/ctor.md` |
| String building | `format!` over `push_str` / `+` | `../rs-patterns/idioms/concat-format.md` |
| Argument types | Borrowed types for arguments | `../rs-patterns/idioms/coercion-arguments.md` |
| Variable built then only read | Temporary mutability | `../rs-patterns/idioms/temporary-mutability.md` |
| Doc examples with long setup | Easy doc initialisation | `../rs-patterns/idioms/rustdoc-init.md` |
| Loops that transform data | Iterator chains (declarative paradigm) | `../rs-patterns/functional/paradigms.md` |

Decision guide for all 52 pages: `../rs-patterns/SKILL.md`.

---

<!-- rules-index:start -->
## Rules (29)

Depth rules under `rules/` — open the one the question needs, not all of them.

**macro-**

- [`macro-export-crate-path`](rules/macro-export-crate-path.md) — Export declarative macros with `#[macro_export]` and a clean import path
- [`macro-fragment-specifiers`](rules/macro-fragment-specifiers.md) — Capture with precise fragment specifiers, not raw `:tt`, where you can
- [`macro-prefer-functions`](rules/macro-prefer-functions.md) — Reach for a macro only when a function or generic cannot express it
- [`macro-private-helpers`](rules/macro-private-helpers.md) — Hide macro-generated helper items behind a `#[doc(hidden)] pub mod __private`
- [`macro-proc-error-spans`](rules/macro-proc-error-spans.md) — Report proc-macro errors as spanned compile errors, never by panicking
- [`macro-proc-syn-quote`](rules/macro-proc-syn-quote.md) — Build procedural macros with `syn`, `quote`, and `proc-macro2`
- [`macro-proc-two-crate`](rules/macro-proc-two-crate.md) — Put procedural macros in a dedicated `proc-macro = true` crate and re-export from the facade
- [`macro-rules-hygiene`](rules/macro-rules-hygiene.md) — Rely on `macro_rules!` hygiene and use `$crate` for paths to your crate's items

**name-**

- [`name-acronym-word`](rules/name-acronym-word.md) — Treat acronyms as words in identifiers: `HttpServer`, not `HTTPServer`
- [`name-as-free`](rules/name-as-free.md) — `as_` prefix: free reference conversion
- [`name-consts-screaming`](rules/name-consts-screaming.md) — Use `SCREAMING_SNAKE_CASE` for constants and statics
- [`name-crate-no-rs`](rules/name-crate-no-rs.md) — Don't suffix crate names with `-rs` or `-rust`
- [`name-funcs-snake`](rules/name-funcs-snake.md) — Use `snake_case` for functions, methods, variables, and modules
- [`name-into-ownership`](rules/name-into-ownership.md) — Use `into_` prefix for ownership-consuming conversions
- [`name-is-has-bool`](rules/name-is-has-bool.md) — Use `is_`, `has_`, `can_`, `should_` prefixes for boolean-returning methods
- [`name-iter-convention`](rules/name-iter-convention.md) — Use iter/iter_mut/into_iter for iterator methods
- [`name-iter-method`](rules/name-iter-method.md) — Name iterator methods `iter()`, `iter_mut()`, and `into_iter()` consistently
- [`name-iter-type-match`](rules/name-iter-type-match.md) — Name iterator types after their source method
- [`name-lifetime-short`](rules/name-lifetime-short.md) — Use short, conventional lifetime names: `'a`, `'b`, `'de`, `'src`
- [`name-no-get-prefix`](rules/name-no-get-prefix.md) — Omit get_ prefix for simple getters
- [`name-to-expensive`](rules/name-to-expensive.md) — Use `to_` prefix for expensive conversions that allocate or compute
- [`name-type-param-single`](rules/name-type-param-single.md) — Use single uppercase letters for type parameters: `T`, `E`, `K`, `V`
- [`name-types-camel`](rules/name-types-camel.md) — Use `UpperCamelCase` for types, traits, and enum names
- [`name-variants-camel`](rules/name-variants-camel.md) — Use `UpperCamelCase` for enum variants

**pat-**

- [`pat-at-bindings`](rules/pat-at-bindings.md) — Use `@` bindings to capture a value while matching it against a pattern
- [`pat-exhaustive-enum`](rules/pat-exhaustive-enum.md) — Match owned enums exhaustively; avoid catch-all `_` that hides new variants
- [`pat-if-let-chains`](rules/pat-if-let-chains.md) — Use `if let` chains to combine pattern bindings and conditions
- [`pat-let-else`](rules/pat-let-else.md) — Use `let ... else` for early-return pattern extraction
- [`pat-matches-macro`](rules/pat-matches-macro.md) — Use `matches!()` for boolean pattern tests

<!-- rules-index:end -->
