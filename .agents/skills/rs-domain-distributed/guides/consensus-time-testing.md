# Consensus, coordination, time and testing

> **rs-domain-distributed · guide** — crate facts verified on crates.io 2026-11-11

## Consensus and coordination — use, don't build

| Need | Use | Notes |
|---|---|---|
| replicated state machine in your own Rust service | **`openraft`** (async Raft, actively maintained; API still evolving — pin versions, read the changelog) | `raft` (tikv/raft-rs) is a lower-level library, last released 2023 |
| config, leader election, leases, service discovery as infrastructure | etcd / Consul / ZooKeeper / Kubernetes Lease objects (`kube` leader election) | external service → timeouts → `Indeterminate` → fail safe |
| a lock for *efficiency* (avoid duplicate work) | `SET NX PX` / advisory lock is fine | duplicates must be harmless |
| a lock for *correctness* | **lease + fencing token**: every lock grant returns a monotonically increasing token; the protected resource rejects requests with an older token | a paused process (GC, VM freeze) can wake up after its lease expired — only the resource can stop it |
| single writer per shard | deterministic partitioning (consistent hashing) so no lock is needed | |

Rules: never implement Raft/Paxos/Redlock for production because "it is simple"; if you must (research, learning), keep it out of the product path and test it with simulation + Jepsen-style checking.

## Time and ordering

- **Durations:** `Instant` (monotonic). **Timestamps for humans/logs:** UTC wall clock. **Never** order events across machines by wall clock.
- **Ids:** UUIDv7 (`uuid` with feature `v7`) or ULID are time-sortable and collision-safe enough for ids; they are not a global order.
- **Order that matters:** per-key sequence numbers, Lamport/hybrid logical clocks (HLC), version vectors for concurrent writers, or a single writer per key.
- **TTL/expiry across machines:** allow skew margins; leases shorter than the failure-detection time are a bug.
- **Last-write-wins** is data loss with extra steps: use versions (compare-and-set), CRDTs, or merge functions.

## Testing distributed behaviour

| Technique | Tool | Finds |
|---|---|---|
| deterministic network simulation (tokio) | **`turmoil`** (tokio-rs) — simulated hosts, partitions, latency, in one process | retry storms, split-brain handling, timeout bugs, reproducible |
| deterministic full-system simulation | **`madsim`** | same, broader (time, randomness, IO) |
| interleaving exploration | `loom` (exhaustive, small), `shuttle` (randomised, larger) | races in your own concurrent code |
| property tests with fault injection | `proptest` + a fault-injecting adapter/decorator (rs-ports-adapters) | idempotency violations, ordering assumptions |
| black-box chaos | containers + network faults (Toxiproxy), Jepsen for stores | what the unit-level fakes cannot (cleanup! rs-cleanup) |

Write at least: *duplicate delivery* test (same message twice → one effect), *crash between steps* test (outbox relay dies after publish), *partition* test (side A cannot reach B for N seconds → documented behaviour, recovery after heal), *clock jump* test, *slow dependency* test (deadline and breaker engage).
