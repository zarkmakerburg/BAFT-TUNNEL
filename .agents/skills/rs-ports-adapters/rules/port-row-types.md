# port-row-types

> Give each adapter its own row/DTO type; domain types carry no vendor derives
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`#[derive(sqlx::FromRow)]` or `#[derive(clickhouse::Row)]` on a domain struct welds the domain to one product and to that product's column types and naming. A separate row type lets each adapter evolve its schema, and lets the domain type enforce invariants the database cannot.

## Bad

```rust
#[derive(Debug, sqlx::FromRow, serde::Serialize)]   // vendor + wire format on the domain type
pub struct Order { pub id: i64, pub status: String }
```

## Good

```rust
// domain
pub struct Order { pub id: OrderId, pub status: OrderStatus }          // newtypes, enums, invariants

// adapters/postgres/orders.rs
#[derive(sqlx::FromRow)]
struct OrderRow { id: i64, status: String }

impl TryFrom<OrderRow> for Order {
    type Error = StoreError;                      // a bad row is Internal/Corrupt, not a panic
    fn try_from(r: OrderRow) -> Result<Self, Self::Error> { /* parse, don't validate */ todo!() }
}
```

## Notes

- `TryFrom` for rows → domain (data can be wrong), `From` for domain → rows.
- API/wire DTOs (HTTP, gRPC) follow the same rule: mapping at the delivery adapter.
- The mapping code is boring and valuable: it is where schema migrations and format quirks are absorbed.

## See Also

- [port-no-vendor-in-domain](port-no-vendor-in-domain.md)
