# dist-no-handrolled-consensus

> Use a maintained consensus/coordination implementation; never roll your own
> **rs-domain-distributed · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Raft/Paxos look small on paper; the bugs live in membership changes, log compaction, disk and network faults — found by Jepsen-style testing, not by unit tests. A production system built on a homemade protocol inherits all of that.

## Do

- Embedded replication → `openraft`; coordination → etcd/Consul/ZooKeeper/Kubernetes Lease via their clients; record the decision (rs-reuse-first).
- Wrap behind a port (`Coordinator`/`LeaderElector`) so it stays replaceable.
- Test with deterministic simulation (`turmoil`/`madsim`) and failure injection.

## See Also

- [dist-fencing-tokens](dist-fencing-tokens.md)
- [rs-reuse-first reuse-search-before-write](../../rs-reuse-first/rules/reuse-search-before-write.md)
