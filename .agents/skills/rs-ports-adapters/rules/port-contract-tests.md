# port-contract-tests

> One generic test suite per port, run by every adapter including the in-memory fake
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Swapping a product is safe only if you can prove the replacement behaves the same. A contract suite turns the port's documentation into executable cases; the in-memory adapter is the reference, real adapters must agree with it. Without it, "we can swap Redis" is a hope.

## Do

```rust
// src/testing/contract.rs — generic over the port, takes a factory
pub async fn audit_store_contract<S: AuditStore>(make: impl Fn() -> S) {
    counts_group_by_utc_day_ascending(&make()).await;
    range_is_half_open(&make()).await;
    empty_range_yields_no_rows(&make()).await;
}

// tests/contract.rs
#[tokio::test] async fn memory()     { audit_store_contract(MemoryAuditStore::default).await; }
#[tokio::test] #[ignore = "needs a container"] async fn clickhouse() { audit_store_contract(real).await; }
```

- Business-logic tests use the in-memory adapter only: fast, no containers (rs-testing).
- Real adapters run the suite in CI against a container (`testcontainers`) — **containers must be removed afterwards** (rs-cleanup: never leave them running; use labels and `--rm`).
- Cases come from edge semantics: ordering, empty results, idempotency, boundary inclusion, TTL expiry, concurrent append, error classification (e.g. unreachable host → `Unavailable`).
- Adding a method to a port = adding its cases in the same commit.

## Bad

Mocking the vendor client in each adapter's unit tests proves your mock, not the product. Use the product (container) or the fake, not both a mock and hope.

## See Also

- [port-swap-drill](port-swap-drill.md)
- [rs-testing](../../rs-testing/SKILL.md)
