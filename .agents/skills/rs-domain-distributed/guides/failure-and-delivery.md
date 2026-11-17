# Failure handling and delivery semantics

> **rs-domain-distributed · guide**

## Delivery guarantees (what you can actually get)

| Guarantee | How | Cost |
|---|---|---|
| at-most-once | send, never retry | may lose messages |
| **at-least-once** | retry until acknowledged | duplicates → consumers must be idempotent |
| "exactly-once effect" | at-least-once + **idempotent processing** (dedup by id) | a dedup store |
| broker-level "exactly-once" (e.g. Kafka transactions) | within the broker's boundary only | doesn't cover your DB/side effects |

## Idempotency keys

```
client ──POST /payments  Idempotency-Key: 7c1e…──▶ server
server: INSERT INTO idempotency(key, request_hash, status, response) … ON CONFLICT (key) DO NOTHING
  - inserted      → execute, store the response, return it
  - exists, done  → return the stored response (same status code and body)
  - exists, in progress → 409/425 or wait
  - exists, different request_hash → 422 (key reuse with another payload)
```
- Key chosen by the **caller** (UUIDv7/random) and reused across its retries; scope per tenant/endpoint; TTL longer than any retry window.
- Store key + outcome **in the same transaction** as the effect (outbox/DB), otherwise a crash between the two re-executes.
- Message consumers: dedup table keyed by `(consumer, message_id)`; process + record in one transaction.
- Natural idempotence first: `PUT` semantics, "set X to V", upserts keyed by business id.

## Timeouts, deadlines and retries

- **Deadline = an absolute time**; convert to a remaining timeout at each hop. gRPC carries it (`grpc-timeout`; tonic `Request::set_timeout`/metadata), HTTP: a custom header (`X-Request-Deadline`) — a downstream call must never outlive its caller.
- **Retry only what is safe**: idempotent methods or operations with idempotency keys; classify errors (transient vs permanent — rs-ports-adapters `port-map-errors`).
- **Backoff with full jitter**: `sleep = random(0, min(cap, base·2^attempt))` (`backon`, `tower::retry`); never fixed intervals (synchronised retries = retry storm).
- **Retry budget:** allow retries as at most ~10% of recent requests; when exhausted, fail fast. Retry at **one** layer (client *or* gateway *or* mesh), not all three (3×3×3 = 27× amplification).
- **Hedged requests** (send a second request after p95) only for idempotent reads with budget.
- **Circuit breaker** around each dependency; half-open probes; fallback behaviour defined (stale data, degrade, fail) — rs-domain-error.
- **Load shedding:** when saturated, reject early (`429`/`503` + `Retry-After`) rather than queue without bound; prioritise health checks and critical traffic; bulkhead per dependency (separate pools/semaphores) so one slow dependency cannot take all threads.

## Consistency vocabulary (say which one you mean)

| Level | Promise |
|---|---|
| linearizable | behaves like one copy, real-time order (consensus-backed stores) |
| sequential / causal | operations seen in an order that respects causality |
| read-your-writes / monotonic reads | per-session guarantees (sticky sessions, version tokens) |
| eventual | replicas converge if writes stop — state the typical and maximum lag |

PACELC reminder: during a partition choose consistency or availability *per operation*; otherwise you trade latency against consistency.
