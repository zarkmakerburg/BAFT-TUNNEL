# sec-zeroize-secrets

> Wipe secrets from memory with a mechanism the optimizer cannot remove — and know its limits
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Keys, tokens and passwords linger in freed heap, stack copies, reallocated buffers and core dumps. A plain `buf.fill(0)` before drop can be **eliminated by the optimizer** as a dead store; `Drop` alone is skipped on `process::exit`/abort. Memory disclosure bugs, crash dumps and swap then leak the secrets.

## Gates

Types holding secrets (`password`, `key`, `token`, `seed`, `private key`, derived keys) are `Clone`, `Debug`, `Serialize`, moved through many functions, or stored in growing buffers (`Vec`/`String`), with no wipe-on-drop.

## Do

| Need | Use |
|---|---|
| wipe on drop that survives optimization | the `zeroize` crate (`Zeroize`, `ZeroizeOnDrop`, `Zeroizing<T>`) — it uses volatile writes and a compiler fence |
| secret wrapper that also redacts `Debug` and requires explicit `expose_secret()` | `secrecy` (`SecretString`, `SecretBox`) |
| growable secret buffers | preallocate (`Vec::with_capacity`) so growth never leaves an un-wiped old buffer; wrap in `Zeroizing<Vec<u8>>` |
| copies | don't derive `Clone`/`Copy`/`Debug`/`Serialize` on secret types; pass `&Secret`; avoid `format!`/`to_string()` of secrets |
| crypto structs | prefer crates that already implement `ZeroizeOnDrop` (RustCrypto, `ring` handles) |

```rust
#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct ApiSecret(Vec<u8>);          // wiped when dropped; no Debug/Clone derived on purpose
```

## Limits (say them in the Design Contract)

Moves are `memcpy`s that can leave stale copies on the stack; swap, hibernation files and core dumps may persist memory (`mlock`/disabling core dumps via crates like `region`/`libc` if the threat model includes them); abort/`exit` skip `Drop`. Zeroizing reduces exposure; it is not a guarantee.

## See Also

- [sec-constant-time-compare](sec-constant-time-compare.md)
- [rs-domain-auth](../../rs-domain-auth/SKILL.md)
