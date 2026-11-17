# async-no-bare-spawn

> Don't fire-and-forget `tokio::spawn`; track the handle or say why it is detached
> **rs-async-patterns · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

A spawned task whose handle is dropped keeps running unsupervised: panics are swallowed, shutdown cannot wait for it, errors vanish.

## Do

- Spawn into a `JoinSet`/`TaskTracker` owned by whoever is responsible for the work, and inspect results (`join_next()` → `JoinError::is_panic()`).
- If detaching is right (a best-effort telemetry flush), comment *why* and make the task log its own errors.
- Need `'static` data? `move` it in or share an `Arc`; borrowed data → `JoinSet` inside a scope that outlives the tasks, or `futures::stream` combinators that do not spawn.

## Bad

```rust
tokio::spawn(async move { let _ = send_audit(event).await; });   // failure and panic invisible, lost on shutdown
```

## See Also

- [async-graceful-shutdown](async-graceful-shutdown.md)
