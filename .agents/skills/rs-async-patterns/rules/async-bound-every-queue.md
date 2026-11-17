# async-bound-every-queue

> Bound every channel, buffer and in-flight set; decide what "full" means
> **rs-async-patterns · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

An unbounded queue turns a slow consumer into an out-of-memory crash hours later, far from the cause. A bound turns it into backpressure — visible, testable, and local.

## Bad

```rust
let (tx, rx) = tokio::sync::mpsc::unbounded_channel();      // producer can outrun the consumer forever
for item in source { tokio::spawn(handle(item)); }          // unbounded tasks
```

## Good

```rust
let (tx, rx) = tokio::sync::mpsc::channel(1024);            // send().await waits when full
let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(64));
// or: stream::iter(source).map(handle).buffer_unordered(64)
```

State the full-queue policy in the Design Contract (wait / drop newest / drop oldest / fail) and test it.

## See Also

- [async-actor-owns-state](async-actor-owns-state.md)
- [rs-concurrency async-bounded-channel](../../rs-concurrency/rules/async-bounded-channel.md)
