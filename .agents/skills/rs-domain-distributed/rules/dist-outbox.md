# dist-outbox

> Write the event in the same transaction as the state change; publish from the outbox
> **rs-domain-distributed · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

"Update DB, then publish" loses the event when the process dies between the two; "publish, then update" announces something that never happened. One transaction + a relay removes the gap.

## Do

- `outbox(id, aggregate_id, type, payload, created_at, published_at)` written inside the repository transaction.
- A supervised relay claims batches (`FOR UPDATE SKIP LOCKED`), publishes, marks; at-least-once → consumers dedup (inbox).
- Alert on outbox age/size; retention job for published rows.
- Hide it behind a port (rs-ports-adapters) so the broker (Kafka/NATS/SQS) is replaceable.

## See Also

- [dist-idempotency-key](dist-idempotency-key.md)
