# port-map-errors

> Classify vendor errors at the boundary; keep the vendor error as `source`, never as a variant
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`Result<_, sqlx::Error>` in a port forces every caller to know sqlx. Worse, retry and alerting logic then differs per backend. A small classified error makes recovery policy backend-independent (rs-domain-error): *transient* → retry with backoff, *rejected* → fix the caller, *conflict* → resolve, *not found* → business decision.

## Bad

```rust
pub enum StoreError { Sqlx(sqlx::Error), Redis(redis::RedisError) }   // callers match on vendors
```

## Good

```rust
#[derive(Debug, thiserror::Error)]
#[error("store {kind:?}: {message}")]
pub struct StoreError {
    pub kind: StoreErrorKind,                 // Unavailable | Timeout | Rejected | Conflict | NotFound | Internal
    pub message: String,
    #[source] pub source: Option<Box<dyn std::error::Error + Send + Sync>>,   // for logs, not for `match`
}

// adapter: the ONE function that reads vendor errors
fn classify(e: VendorError) -> StoreError { /* table: network → Unavailable, deadline → Timeout, … */ }
```

## Rules of Thumb

- One `classify` function per adapter, covered by a table test of vendor error → kind.
- `is_transient()` on the port error; retry/circuit-breaker policy sits in the caller or a decorator, not in each adapter.
- Don't invent a kind for every vendor error; 4–7 kinds that callers can *act on* are enough.
- Log the `source` chain (rs-observability); never show it to end users.

## See Also

- [port-contract-tests](port-contract-tests.md)
