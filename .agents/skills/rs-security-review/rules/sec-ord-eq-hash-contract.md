# sec-ord-eq-hash-contract

> Eq, Ord and Hash must agree — and keys must not change while they are in a collection
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`HashMap`, `BTreeMap`, `sort`, `binary_search` and `dedup` assume `Eq`/`Ord`/`Hash` are consistent. Violations are *logic errors*: lookups that miss, duplicate keys, infinite loops, and — in recent standard libraries — `sort` panics on a non-total order. For security-relevant maps (sessions, permissions, rate limits, dedup/idempotency keys) a missed lookup means a missed check.

## Gates

- A hand-written `PartialEq`/`Ord`/`Hash` (or derive on a type with `f32`/`f64`, interior mutability, or custom comparison) used as a map/set key or in sorting.
- A key type with interior mutability (`Cell`, `RefCell`, `Mutex`, atomics) mutated after insertion.

## Rules

| Rule | Detail |
|---|---|
| `a == b` ⇒ `hash(a) == hash(b)` | hash exactly the fields equality compares (case-insensitive equality needs case-folded hashing) |
| `Ord` is a total order consistent with `Eq` | floats: wrap or sort with `f64::total_cmp`; never `partial_cmp(..).unwrap()` (NaN panics) |
| keys are immutable while stored | no interior mutability that affects `Eq/Ord/Hash`; to change a key, remove and reinsert |
| don't derive `Hash`/`Eq` on types containing `f64` | use ordered wrappers or integer representations |
| comparisons of secrets are separate | use constant-time equality ([sec-constant-time-compare](sec-constant-time-compare.md)), not `Eq` on keys |

```rust
pub fn sort_f64(v: &mut [f64]) { v.sort_by(f64::total_cmp); }       // NaN sorts consistently
```

## Unsafe code and hostile impls

Generic `unsafe` code must **not** rely on `Ord`, `Eq`, `Hash`, `Clone`, `Deref`, `Borrow` or `ExactSizeIterator::len` being correct — all are safe traits that may lie or panic. Treat user-supplied impls as adversarial for memory safety; closures may panic mid-operation (panic safety: leave the container valid at every `unwind` point).

## See Also

- [sec-constant-time-compare](sec-constant-time-compare.md)
- [rs-unsafe](../../rs-unsafe/SKILL.md)
