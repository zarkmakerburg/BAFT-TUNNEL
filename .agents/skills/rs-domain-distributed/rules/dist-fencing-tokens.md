# dist-fencing-tokens

> A lock without a fencing token cannot guarantee correctness
> **rs-domain-distributed · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

A lock holder can pause (GC, VM freeze, network) past its lease; another process acquires the lock; the first wakes up and writes anyway. Only the *protected resource* can reject the stale writer.

## Do

- Each acquisition returns a **monotonically increasing token**; the resource stores the highest seen and rejects lower tokens.
- Use locks for correctness only with fencing; otherwise they are an efficiency optimisation and duplicates must be harmless.
- Prefer designs that need no lock: partition by key (single writer), idempotent operations, compare-and-set with versions.

## Bad

```rust
if redis.set_nx("lock:job", id, ttl).await? { do_job_and_write_db().await?; }     // paused past ttl → two writers
```

## See Also

- [dist-no-handrolled-consensus](dist-no-handrolled-consensus.md)
