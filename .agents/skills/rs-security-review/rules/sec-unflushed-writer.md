# sec-unflushed-writer

> Write errors vanish unless you flush: BufWriter, File and discarded Results
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`BufWriter`'s `Drop` flushes but **ignores errors**; `File` drop does not `fsync`; `let _ = out.write_all(...)` throws the result away. Audit logs, ledgers, config/state saves and "delivered" acknowledgements can be silently lost or truncated — and the program reports success.

## Gates

A write/send/flush/save result is discarded (`let _ =`, `.ok()`, ignoring `Result` from `write_all`, `send`, `post`, `set_body`), or a `BufWriter`/`LineWriter`/compressor/encoder is dropped without an explicit `flush()`/`finish()`/`into_inner()`, on a path where the data matters.

## Do

```rust
let mut w = BufWriter::new(File::create(path)?);
w.write_all(&bytes)?;
w.flush()?;                         // surfaces I/O errors
w.get_ref().sync_all()?;            // durability, when it matters (audit logs, state files)
```
- Handle or propagate every `Result`; `#[must_use]` types exist for a reason; enable `clippy::let_underscore_must_use` and `unused_must_use` as errors on security paths.
- Encoders (gzip, zstd, tar builders): call their `finish()`/`into_inner()`; dropping them may leave a truncated archive.
- Atomic replace: temp file → write → flush → `sync_all` → `rename`.
- Network "fire and forget" that is actually required (audit, billing) goes through a durable queue with retries, not `let _ = tx.send(..)` (rs-domain-distributed outbox).
- Fail **closed** when an audit write fails for a high-risk action (rs-domain-policy `policy-decision-log`).

## See Also

- [sec-drop-panic-and-skip](sec-drop-panic-and-skip.md)
- [rs-error-handling err-fail-closed](../../rs-error-handling/rules/err-fail-closed.md)
