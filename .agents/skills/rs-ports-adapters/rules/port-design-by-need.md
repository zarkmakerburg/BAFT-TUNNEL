# port-design-by-need

> Shape the port from the use case in business words, not from the product's API
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

A port copied from a product's API is the product in disguise: replacing it needs the same concepts in the replacement. A port shaped by what the business needs can be implemented by almost anything.

## Bad

```rust
pub trait Db {
    async fn execute(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>, sqlx::Error>;
}
pub trait Cache {
    async fn redis_get(&self, key: &str) -> redis::RedisResult<Option<String>>;   // product name in the method
}
```

## Good

```rust
pub trait AuditStore: Send + Sync {
    fn append(&self, events: Vec<AuditEvent>) -> impl Future<Output = Result<(), StoreError>> + Send;
    fn daily_counts(&self, range: TimeRange) -> impl Future<Output = Result<Vec<DayCount>, StoreError>> + Send;
}
pub trait SessionCache: Send + Sync {
    fn get(&self, id: &SessionId) -> impl Future<Output = Result<Option<Session>, CacheError>> + Send;
    fn put(&self, s: Session, ttl: Duration) -> impl Future<Output = Result<(), CacheError>> + Send;
}
```

## Checks

- Method names are verbs from the business (`open_for`, `daily_counts`), not the product (`select`, `hset`).
- Arguments/returns are domain types (newtypes, enums), not `String` + `serde_json::Value`.
- Semantics are documented on the trait: ordering, atomicity ("all or none"), idempotency, TTL meaning, empty results.
- Keep traits **narrow** (interface segregation): a `Reader` and a `Writer` port beat one 25-method `Storage`.
- Pagination, time ranges, filters are small domain types (`Page`, `TimeRange`, `OrderFilter`) the adapter translates into its query language.

## See Also

- [port-no-vendor-in-domain](port-no-vendor-in-domain.md)
- [port-yagni](port-yagni.md)
