# sec-constant-time-compare

> Compare secrets and MACs in constant time
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`==` on byte slices returns at the first difference; response time then reveals how many leading bytes matched, letting an attacker recover a token or MAC byte by byte over many requests.

## Gates

Equality of an API key, session token, MAC/signature, password hash, reset token or CSRF token using `==`, `!=`, `starts_with`, `memcmp`, or `String` comparison.

## Do

```rust
use subtle::ConstantTimeEq;
pub fn tokens_equal(a: &[u8], b: &[u8]) -> bool { a.ct_eq(b).into() }   // length differences still leak — compare digests of equal length
```
- `subtle::ConstantTimeEq`, `ring::constant_time::verify_slices_are_equal`, or the verify functions of MAC crates (`hmac`'s `verify_slice`) — they compare in constant time.
- Hash both sides to a fixed length first (e.g. HMAC or SHA-256 of the token) when lengths may differ.
- Don't branch, index or divide on secret values in code you write yourself; for primitives use audited crates rather than hand-rolled implementations (rs-reuse-first).
- Constant-time compilation is not guaranteed by the language; for crypto implementers, test with statistical timing tools (dudect-style) — out of scope for application code.

## False positives

Comparing non-secret identifiers (user ids, public keys); lookups by public id before the constant-time secret comparison.

## See Also

- [rs-domain-auth](../../rs-domain-auth/SKILL.md)
- [sec-zeroize-secrets](sec-zeroize-secrets.md)
