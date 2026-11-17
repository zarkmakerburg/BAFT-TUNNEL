# sec-resource-exhaustion

> Never size memory, time or connections from an unvalidated number the peer controls
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Resource exhaustion is the most common remote DoS in Rust services: memory-safe code can still be asked to allocate 4 GiB, decompress a 1 KiB bomb, hold 100 000 sockets or run a quadratic algorithm on crafted keys. OOM is an abort, not a panic.

## Gates

A length, count, depth or rate comes from outside **and** drives an allocation, loop bound, buffer, queue, cache, regex, hash table or connection pool — with no upper bound.

## Checklist

| Sink | Bad | Do |
|---|---|---|
| `Vec::with_capacity(n)`, `vec![0; n]`, `String::with_capacity` from a wire length | trusts `n` | cap `n` to a protocol maximum **and** to bytes actually remaining; allocate as data arrives |
| `read_to_end`, `read_to_string`, `body.collect()` | unbounded | `Read::take(limit + 1)` then reject if over; framework body limits (`axum::DefaultBodyLimit`, `RequestBodyLimitLayer`) |
| decompression (gzip, zip, zstd, brotli) | output size unchecked | limit the *decompressed* size and the ratio; stream |
| collections keyed by attacker input | unbounded growth (caches, maps, sets, metrics label cardinality) | max entries, TTL, eviction (`moka`), cap label values |
| hashing attacker keys | non-randomized fast hashers (e.g. FxHash) enable collision floods | keep `std` SipHash/randomized hashers for untrusted keys |
| regex from users | catastrophic patterns in backtracking engines (`fancy-regex`, PCRE bindings) | `regex` crate (linear time) with `size_limit`/`dfa_size_limit`, or no user regex |
| connections, tasks, streams | one task/socket per request without limit | `Semaphore`, concurrency limit layers, per-peer quotas, idle timeouts, header/size/time limits (slowloris) |
| nested/amplifying formats (XML entities, YAML anchors, JSON → big structs) | expansion | entity/alias limits, size after expansion |
| arithmetic on sizes (`count * size`) | wrap in release | `checked_mul` → error (see sec-panic-on-input) |

## Good

```rust
pub fn read_limited(r: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.take(limit + 1).read_to_end(&mut buf)?;                     // reads at most limit+1 bytes
    if buf.len() as u64 > limit { return Err(io::Error::new(io::ErrorKind::InvalidData, "input too large")); }
    Ok(buf)
}
```

## False positives

Sizes validated against a protocol maximum upstream; trusted internal producers; bounds enforced by the framework (verify the default and that it is not raised).

## See Also

- [sec-recursion-depth-dos](sec-recursion-depth-dos.md)
- [rs-async-patterns async-bound-every-queue](../../rs-async-patterns/rules/async-bound-every-queue.md)
