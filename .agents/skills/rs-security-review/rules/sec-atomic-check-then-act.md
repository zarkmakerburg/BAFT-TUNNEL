# sec-atomic-check-then-act

> Atomic operations are atomic one at a time — compound invariants need one operation or a lock
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`if counter.load() > 0 { counter.fetch_sub(1) }` is two atomics: two threads both pass the check. Security-relevant uses — rate limits, quotas, one-time tokens, "first writer wins", permits — silently over-grant.

## Gates

A decision is made on an atomic's loaded value, and a later atomic or non-atomic operation relies on it still being true; or several atomics together form one invariant.

## Do

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

/// Take one permit if any remain; a load-then-store would let two threads both succeed.
pub fn try_acquire(counter: &AtomicUsize) -> bool {
    // `fetch_update` loops on compare_exchange; newer toolchains also offer `try_update` — use what your MSRV has
    counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1)).is_ok()
}
```
- One read-modify-write per decision: `fetch_update`, `compare_exchange(_weak)` in a loop, `swap`, `fetch_*`.
- Several fields that must change together → a `Mutex`/`RwLock` around the struct (or an immutable snapshot swapped with `arc-swap`).
- `Relaxed` is fine for independent counters/statistics; anything that publishes data to another thread needs `Release`/`Acquire` (rs-concurrency `conc-atomic-ordering`).
- `unsafe impl Sync/Send` on types with interior mutability: audit every access path (rs-unsafe `safety-05-send-sync`).
- `static mut` is a data race by default — use atomics/`OnceLock`/`Mutex`.

## See Also

- [sec-lock-order-and-reentrancy](sec-lock-order-and-reentrancy.md)
