# port-uow-transactions

> Keep transactions inside the port: one aggregate per call, or an explicit unit-of-work closure
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Leaking `Transaction` types through the domain re-couples it to one database. Transactions are an adapter concern, but business rules sometimes need atomicity across steps.

## Options (in order of preference)

| Need | Pattern |
|---|---|
| change one aggregate atomically | the repository's `save(&aggregate)` is the transaction |
| several changes that must commit together in one store | a port method that takes the whole change (`apply(ChangeSet)`), or `with_transaction(|tx| async { … })` where `tx` is a port-defined trait object/generic |
| writes to a store **and** a broker | **outbox**: write events to the store in the same transaction, a relay publishes them (rs-domain-error for retries) — no distributed transaction through ports |
| two different products must stay in sync | eventual consistency + idempotent consumers; state it in the port docs |

## Bad

```rust
pub trait OrderRepo { async fn begin(&self) -> sqlx::Transaction<'_, sqlx::Postgres>; }   // vendor type in the port
```

## Good

Document the atomicity guarantee on each port method and test it in the contract suite (e.g. a failed `apply` leaves nothing behind).

## See Also

- [port-contract-tests](port-contract-tests.md)
