# sec-serialization-integrity

> Serialization must be total, deterministic and agree with its own length claims
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Signatures, hashes, caches, replicated state and idempotency keys are computed over serialized bytes. If two honest nodes (or two runs) serialize the same value differently, verification fails or state diverges; if a hand-written `Serialize` lies about its length, length-prefixed formats corrupt the stream.

## Gates

- A hand-written `Serialize`/`Deserialize` (or `serialize_with`), especially `serialize_struct`/`serialize_map` with a declared length.
- Serialized bytes feed a signature, MAC, hash, cache key, consensus/replication or comparison.

## Rules

| Hazard | Fix |
|---|---|
| declared length ≠ elements written (`serialize_struct("S", 3)` then 2 fields; conditional `skip_serializing_if` fields counted wrongly) | count what is actually emitted; derive instead of hand-writing; test with every format you support |
| `HashMap`/`HashSet` iteration order is randomized per process | use `BTreeMap`/`IndexMap` (or sort) wherever bytes are hashed, signed, diffed or replicated |
| floats: NaN payloads, `-0.0`, formatting differences | avoid floats in signed/consensus data; canonical encoding or integers (fixed-point) |
| JSON "same value, different bytes" (key order, whitespace, number forms) | canonical JSON (RFC 8785) before signing, or sign a binary canonical encoding |
| deserialize ≠ serialize round trip | property test `decode(encode(x)) == x` (proptest) and fuzz the decoder (rs-fuzzing) |
| unknown fields silently ignored on security data | `#[serde(deny_unknown_fields)]` (rs-ecosystem `serde-deny-unknown-fields`) |
| time/ids/randomness inside replicated state transitions | pass them in as inputs; state machines must be deterministic (same inputs ⇒ same state) |

## See Also

- [rs-ecosystem serde-try-from-validate](../../rs-ecosystem/rules/serde-try-from-validate.md)
- [rs-domain-distributed](../../rs-domain-distributed/SKILL.md)
