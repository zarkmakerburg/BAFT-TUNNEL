# sec-drop-panic-and-skip

> Drop must not panic, and cleanup must not depend on a Drop that process::exit, mem::forget or abort will skip
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

A panic inside `Drop` while already unwinding is a **double panic → abort**. Conversely, `std::process::exit`, `std::process::abort`, `mem::forget`, leaked `Rc` cycles and `panic = "abort"` skip destructors entirely — so flushes, lock releases, audit-log writes and temp-file cleanup that live only in `Drop` silently never happen.

## Gates

- A `Drop` impl (or a type it owns) can panic: `unwrap`/`expect`, indexing, `RefCell` borrow, locking a poisoned mutex, I/O with `?`-less `unwrap`.
- Required cleanup (flush, commit, unlock, audit record, deleting secrets) lives only in `Drop` and the program can exit via `process::exit`/abort, or builds with `panic = "abort"`.

## Do

- `Drop` is infallible: log and swallow errors; offer an explicit fallible `close()`/`finish()`/`commit()` for callers that need the result, and make `Drop` do best-effort only.
- Avoid locking and allocating in `Drop` on hot error paths; never panic.
- Critical cleanup: call it explicitly on every exit path (graceful shutdown — rs-async-patterns), flush logs/audit before `process::exit`, and don't `mem::forget` guards.
- `BufWriter` ignores flush errors on drop — see [sec-unflushed-writer](sec-unflushed-writer.md).
- Secrets: don't rely on `Drop` alone if abort/exit is possible ([sec-zeroize-secrets](sec-zeroize-secrets.md)).

## Bad

```rust
impl Drop for Session { fn drop(&mut self) { self.conn.lock().unwrap().rollback().unwrap(); } }   // poisoned lock or I/O error → double panic → abort
fn main() { let _g = AuditFlush::new(); work(); std::process::exit(1); }                       // destructor never runs
```

## See Also

- [rs-lifecycle](../../rs-lifecycle/SKILL.md)
- [sec-unflushed-writer](sec-unflushed-writer.md)
