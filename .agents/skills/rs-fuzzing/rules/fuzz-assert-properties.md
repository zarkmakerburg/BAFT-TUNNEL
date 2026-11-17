# fuzz-assert-properties

> Fuzz for properties (round trip, invariants, equivalence), not only for crashes
> **rs-fuzzing · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

"Doesn't panic" misses silent corruption: a parser that accepts malformed input, an encoder that changes bytes, a fast path that disagrees with the reference. Those are the bugs that become security issues.

## Do

- Round trip: `encode(decode(x)) == x[..used]` for canonical formats; `decode(encode(v)) == v` for generated values.
- Differential: compare with a simple reference or with another adapter of the same port (rs-ports-adapters contract suite as oracle).
- Invariants after each step of an operation sequence (state machines): totals, uniqueness, isolation between tenants.
- Security oracles: masked output never contains the secret; a denied request never reaches the upstream.
- Budget oracles: no input takes longer than a bound or allocates more than a bound.

## See Also

- [fuzz-harness-is-a-pure-function](fuzz-harness-is-a-pure-function.md)
- [rs-security-review sec-resource-exhaustion](../../rs-security-review/rules/sec-resource-exhaustion.md)
