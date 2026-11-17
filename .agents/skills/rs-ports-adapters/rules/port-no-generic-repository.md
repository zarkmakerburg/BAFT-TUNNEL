# port-no-generic-repository

> Don't write `Repository<T>` with `find/save/delete` for everything
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

A generic CRUD trait looks like decoupling but is the table API in disguise: it cannot express the queries the business needs (`open_orders_for(customer)`), so callers filter in memory or smuggle in query strings — and replacing the product still changes every caller. It also gives aggregates no consistency boundary.

## Bad

```rust
pub trait Repository<T> {
    async fn find(&self, id: i64) -> Option<T>;
    async fn save(&self, t: &T);
    async fn delete(&self, id: i64);
    async fn find_where(&self, filter: &str) -> Vec<T>;      // a query language again
}
```

## Good

```rust
pub trait OrderRepository: Send + Sync {
    fn by_id(&self, id: OrderId) -> impl Future<Output = Result<Option<Order>, StoreError>> + Send;
    fn open_for(&self, c: CustomerId, page: Page) -> impl Future<Output = Result<Vec<Order>, StoreError>> + Send;
    fn save(&self, o: &Order) -> impl Future<Output = Result<(), StoreError>> + Send;   // one aggregate per call
}
```

One repository per **aggregate** (rs-domain-model), with only the operations the use cases need. Share *implementation* helpers inside an adapter, not the trait.

## See Also

- [port-design-by-need](port-design-by-need.md)
- [rs-domain-model](../../rs-domain-model/SKILL.md)
