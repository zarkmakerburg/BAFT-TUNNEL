# dist-retry-budget

> Retry with jitter, within a budget, at one layer
> **rs-domain-distributed · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Retries multiply load exactly when a dependency is struggling. Three layers each retrying three times is 27× traffic; fixed intervals synchronise clients into waves.

## Do

- Exponential backoff with **full jitter**; cap attempts and total time (≤ the caller's deadline).
- A **budget**: retries ≤ ~10% of requests over a window; beyond it, fail fast.
- Only retry transient errors and idempotent operations (or keyed ones).
- Choose **one** layer (client library, gateway or mesh) and disable the rest.
- Circuit-break per dependency; define the degraded behaviour.

## Bad

```rust
loop { if call().await.is_ok() { break } tokio::time::sleep(Duration::from_secs(1)).await }   // unbounded, fixed, synchronised
```

## See Also

- [dist-deadline-propagation](dist-deadline-propagation.md)
