# Transactional outbox, inbox and sagas

> **rs-domain-distributed · guide**

## Outbox (publish what you committed)

```sql
-- same database transaction as the state change
BEGIN;
UPDATE orders SET status = 'PAID' WHERE id = $1;
INSERT INTO outbox(id, aggregate_id, type, payload, created_at) VALUES ($2, $1, 'order.paid', $3, now());
COMMIT;
-- relay (separate task/process): poll or logical-decode, publish to the broker, mark/delete
SELECT id, type, payload FROM outbox WHERE published_at IS NULL ORDER BY id LIMIT 100 FOR UPDATE SKIP LOCKED;
```
- Relay is **at-least-once**: a crash after publish and before mark re-publishes → consumers dedup by event `id` (inbox table).
- Ordering: per aggregate (partition key = aggregate id); don't promise global order.
- Rust shape: `OutboxStore` port with `append(tx, event)` (called inside the repository's transaction — rs-ports-adapters `port-uow-transactions`) and `claim_batch/mark_published`; relay = a supervised task with batching, backoff and graceful drain (rs-async-patterns).
- Alternatives: CDC (logical decoding/Debezium) instead of polling; the contract is the same.
- Clean up: retention job for published rows; alert on outbox age (a growing outbox = a stuck relay).

## Inbox (consume once)

```sql
INSERT INTO inbox(consumer, message_id, received_at) VALUES ($1, $2, now()) ON CONFLICT DO NOTHING;
-- 0 rows inserted → duplicate: ack and skip; 1 row → handle in the same transaction, then commit
```

## Sagas

| Style | How | Pros | Cons |
|---|---|---|---|
| **orchestration** | a coordinator (state machine persisted in a DB) tells each participant what to do and records progress | explicit flow, one place to look, timeouts and retries central | the orchestrator is a component to run |
| **choreography** | services react to each other's events | no coordinator | the flow is implicit; hard to see and debug as it grows |

Design rules:
- Each step has a **compensation** (undo/cancel) that is itself idempotent; some steps cannot be undone → order them last ("pivot": after it succeeds, only forward recovery).
- **Persist saga state** after every step; recover by replaying from the last durable state (a crash must not lose or duplicate steps).
- **Timeouts** per step and for the saga; define the outcome of a timeout (compensate / escalate to a human).
- Compensation can fail: retry with backoff, then park for manual action with an alert.
- Model the state machine as an enum with explicit transitions (rs-type-driven: type state for compile-time-checked transitions where it pays).
- Observability: one saga id on every message/log line; a dashboard of sagas by state/age.

## Not 2PC

Two-phase commit across independent services/brokers needs a coordinator that is itself a single point of failure and holds locks across network round trips. Within one database/engine, use its transactions; across systems use outbox + sagas + reconciliation.
