# dist-idempotency-key

> Retried non-idempotent operations carry a caller-chosen key, stored with the effect
> **rs-domain-distributed · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Timeouts make retries unavoidable; retries make duplicates unavoidable. Without a key, "charge the card" runs twice.

## Do

- Caller generates a key (UUIDv7/random) per logical operation and reuses it on every retry.
- Server stores `key → request hash + outcome` in the **same transaction** as the effect; a repeat returns the stored outcome; the same key with a different payload is rejected.
- TTL ≥ longest retry window. Message consumers dedup on `(consumer, message_id)`.
- Prefer naturally idempotent shapes (`PUT`, upsert by business key).

## Bad

```rust
async fn pay(req: PayRequest) -> Result<Receipt, E> { gateway.charge(req).await }     // POST + retry = double charge
```

## See Also

- [dist-outbox](dist-outbox.md)
- [rs-domain-data data-short-transactions](../../rs-domain-data/rules/data-short-transactions.md)
