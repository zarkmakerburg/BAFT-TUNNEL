---
name: rs-domain-distributed
description: "Use when a design spans processes or machines — retries, timeouts, idempotency, outbox/inbox, sagas, distributed transactions, consensus, leader election, distributed locks, clocks and ordering, delivery guarantees, partitions, deadlines, load shedding, deterministic simulation testing. Triggers: distributed system, microservices consistency, exactly once, at-least-once, at-most-once, idempotent, idempotency key, retry storm, retry budget, timeout, deadline propagation, outbox, inbox, transactional outbox, saga, compensation, two-phase commit, 2PC, Raft, openraft, consensus, leader election, lease, fencing token, distributed lock, Redlock, CAP, PACELC, eventual consistency, linearizable, causal, Lamport, hybrid logical clock, vector clock, UUIDv7, ULID, clock skew, partition, split brain, turmoil, madsim, deterministic simulation, Jepsen, load shedding, circuit breaker, bulkhead"
user-invocable: false
---

# Distributed Systems — design for the network you actually have

> **Layer 3: Domain** · what a service promises when machines crash, networks partition and clocks disagree
>
> Depth: `guides/failure-and-delivery.md` · `guides/outbox-saga.md` · `guides/consensus-time-testing.md` · never hand-roll consensus (rs-reuse-first)

## Core Question

**If this call times out, what do I know — and is it safe to try again?**

A timeout means *unknown*: the request may have been lost, executed, or executed and the reply lost. Every distributed design starts from that sentence.

## The failure model

| Failure | Looks like | Design response |
|---|---|---|
| crash | connection reset, no reply | restart, recover from durable state, idempotent replay |
| slow | latency spike | deadlines, retry budget, load shedding, bulkheads |
| lost / duplicated / reordered messages | missing or repeated delivery | idempotency keys, sequence numbers, dedup |
| partition | two sides alive, cannot talk | choose per operation: refuse (consistency) or proceed and reconcile (availability) |
| clock skew | timestamps disagree | monotonic clocks for durations; logical/hybrid clocks for order; never "last write wins by wall clock" for correctness |
| partial failure | one of three steps done | outbox/saga, compensation, reconciliation jobs |

## Decision table

| Need | Use | Not |
|---|---|---|
| "exactly once" effect | **at-least-once delivery + idempotent handler** (idempotency key, dedup table, `INSERT … ON CONFLICT DO NOTHING`) | hoping the broker gives exactly-once end to end |
| update DB **and** publish an event | **transactional outbox**: event row in the same DB transaction, a relay publishes | write DB then publish (or the reverse) and hope |
| multi-step business transaction across services | **saga** (orchestrated or choreographed) with compensations and timeouts | distributed 2PC across heterogeneous systems |
| agree on a value / elect a leader / replicate a log | a **consensus library or service** (`openraft`, etcd, Consul, ZooKeeper) | your own Raft/Paxos |
| mutual exclusion across processes | lease + **fencing token** checked by the resource; or avoid with partitioning/idempotency | `SET NX` locks as correctness guarantee; Redlock |
| ordering of events | per-key ordering (partition by key), sequence numbers; **UUIDv7/ULID** for time-sortable ids; HLC for causal timestamps | comparing wall clocks of different machines |
| calling a flaky dependency | deadline + capped retries with backoff & jitter + circuit breaker (rs-domain-error); retries **only for idempotent** operations or with idempotency keys | unlimited retries, retries at every layer |
| protecting yourself | bounded queues, concurrency limits, **load shedding** (reject early with 429/503), bulkheads per dependency | accepting everything and falling over |

## Rules

| Rule | Detail |
|---|---|
| **Every network call has a deadline** and the remaining deadline is **propagated** downstream (`dist-deadline-propagation`) |
| **Retry budget, not retry count**: cap retries to a fraction of traffic; exponential backoff with full jitter; retry at **one** layer only (`dist-retry-budget`) |
| **Idempotency keys** for every non-idempotent operation exposed to retries (`dist-idempotency-key`) |
| **Outbox for DB+event**; consumers dedup (`dist-outbox`) |
| **Don't hand-roll consensus or distributed locks** (`dist-no-handrolled-consensus`, `dist-fencing-tokens`) |
| **State the consistency level** of each read/write path in the port docs (linearizable / read-your-writes / eventual within N s) |
| **Reconciliation exists**: jobs that detect and repair divergence (ledger vs. events, DB vs. index) |
| **Everything carries a trace/correlation id** across hops (rs-observability) |

## Trace

| Direction | Go to |
|---|---|
| ↑ Up | rs-domain-cloud-native (deployment, health, graceful shutdown), rs-domain-fintech (ledgers need strong guarantees), rs-domain-iot (unreliable links) |
| ↓ Down | rs-domain-error (retry/circuit breaker), rs-async-patterns (timeouts, cancellation, backpressure), rs-domain-data (transactions, idempotent writes, outbox table), rs-ports-adapters (broker/store behind ports, outbox as a port) |
| → Delivery | rs-testing (deterministic simulation with `turmoil`/`madsim`, `shuttle`/`loom`, property tests), rs-observability (trace propagation), rs-review (security & contract pass: every external call has a deadline and an idempotency story) |

<!-- rules-index:start -->
## Rules (6)

Depth rules under `rules/` — open the one the question needs, not all of them.

**dist-**

- [`dist-deadline-propagation`](rules/dist-deadline-propagation.md) — Every call has a deadline, and downstream calls inherit what is left
- [`dist-fencing-tokens`](rules/dist-fencing-tokens.md) — A lock without a fencing token cannot guarantee correctness
- [`dist-idempotency-key`](rules/dist-idempotency-key.md) — Retried non-idempotent operations carry a caller-chosen key, stored with the effect
- [`dist-no-handrolled-consensus`](rules/dist-no-handrolled-consensus.md) — Use a maintained consensus/coordination implementation; never roll your own
- [`dist-outbox`](rules/dist-outbox.md) — Write the event in the same transaction as the state change; publish from the outbox
- [`dist-retry-budget`](rules/dist-retry-budget.md) — Retry with jitter, within a budget, at one layer

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|---|---|
| Retry/backoff/circuit breaker | rs-domain-error |
| Timeouts, cancellation, shutdown | rs-async-patterns |
| Transactions, outbox table, idempotent writes | rs-domain-data |
| Picking a consensus library or broker client | rs-reuse-first |
