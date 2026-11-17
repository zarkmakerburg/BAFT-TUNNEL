# sec-lock-order-and-reentrancy

> Lock scopes, ordering and re-entrancy: double locks, ABBA, Condvar, call_once, signal handlers
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Rust prevents data races on safe code, not deadlocks, livelocks or lost wake-ups. A service that deadlocks under a crafted request sequence is a DoS.

## Gates and fixes

| Hazard | Shape | Fix |
|---|---|---|
| **double lock** | `let g = m.lock(); … helper()` where `helper` locks `m` again (guards live to end of scope) | shorten guard scope (`{ … }` or `drop(g)`), pass `&mut T` down instead of re-locking, or use a re-entrant design |
| **ABBA** | thread 1 locks A then B, thread 2 B then A | one global lock order, documented in the Design Contract; or a single lock over both |
| **Condvar** | `wait` without a predicate loop, notify before wait, wrong mutex | `while !cond { guard = cv.wait(guard)? }` (spurious wake-ups), set the condition under the mutex before `notify_*` |
| **`Once::call_once` / `LazyLock` re-entrancy** | initialiser calls code that initialises the same cell | no cyclic initialisation; keep initialisers simple |
| **poisoned locks** | `lock().unwrap()` after a panic elsewhere → cascading panics | decide policy: recover (`into_inner`) or fail closed; `parking_lot` has no poisoning |
| **lock across `.await`** | guard held over suspension | restructure (rs-concurrency `async-no-lock-await`) |
| **channels** | bounded channel with producer and consumer in the same task; consumer waits on producer that waits on consumer | separate tasks, bounded queues with explicit full policy |
| **signal handlers / callbacks** | handler allocates, locks, or logs | only async-signal-safe operations: set an atomic flag / write to a self-pipe, handle in the main loop |

Tooling: `loom` (exhaustive interleavings for small units), `shuttle` (randomised for larger), and `parking_lot`'s `deadlock_detection` feature to find cycles at runtime in tests.

## See Also

- [sec-atomic-check-then-act](sec-atomic-check-then-act.md)
- [rs-concurrency](../../rs-concurrency/SKILL.md)
