# sec-panic-on-input

> A panic reachable from input is a denial of service: no unwrap/index/slice/assert/overflow on untrusted data
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

In a server one malformed request that panics kills a worker (or the process with `panic = "abort"`). Safe Rust turns memory errors into panics — which is good for safety and bad for availability if the trigger is attacker-controlled.

## Gates

Input (network, file, env, IPC, FFI) can reach an `unwrap()`, `expect()`, slice/array index, `&s[a..b]`, `assert!`/`unreachable!`, `RefCell` re-borrow, `Option`-assuming arithmetic, `.remove(i)`/`.split_at(n)`/`.swap_remove`, or integer overflow/underflow.

## Patterns and fixes

| Trigger | Fix |
|---|---|
| `x.unwrap()` / `expect` on parsed input | `?`/`ok_or`, return a typed error; `unwrap` only for proven invariants with a comment |
| `v[i]`, `&buf[a..b]` with input-derived `i/a/b` | `v.get(i)`, `buf.get(a..b)`, validate first |
| `&s[..n]` on `&str` | panics inside a multi-byte char — `s.get(..n)` or the boundary loop below |
| `a + b`, `a - b`, `a * b` on input-derived integers | debug builds panic, **release builds wrap silently** — `checked_*` / `saturating_*` / `try_from` and return an error; consider `overflow-checks = true` in release for services |
| `n as usize` / `as u32` | silently truncates/sign-changes — `usize::try_from(n)?` |
| `assert!`/`debug_assert!` on input conditions | assertions are for invariants, not validation; `debug_assert!` vanishes in release |
| `RefCell::borrow_mut` in re-entrant callbacks | `try_borrow_mut`, or restructure ownership |
| `Duration`/`Instant` arithmetic (`a - b`) | `checked_sub`, `saturating_duration_since` |

```rust
/// Truncate to at most `max` bytes without splitting a UTF-8 character.
pub fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max { return s; }
    let mut end = max;
    while !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
}
```

## Containment (defence in depth, not a fix)

`std::panic::catch_unwind` per request / `tower-http::CatchPanicLayer` returns 500 instead of killing the worker — **only under `panic = "unwind"`**; it does nothing for aborts (stack overflow, OOM, `panic = "abort"`). Fix the cause too.

## False positives

Input validated to the needed range immediately upstream (trace it); invariants established by construction (`NonZeroU32`, array length types); test-only code.

## See Also

- [sec-recursion-depth-dos](sec-recursion-depth-dos.md)
- [rs-type-driven num-overflow-explicit](../../rs-type-driven/rules/num-overflow-explicit.md)
