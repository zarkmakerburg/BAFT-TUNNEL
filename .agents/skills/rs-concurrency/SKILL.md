---
name: rs-concurrency
description: "CRITICAL: Use for concurrency/async. Triggers: E0277 Send Sync, cannot be sent between threads, thread, spawn, channel, mpsc, Mutex, RwLock, Atomic, async, await, Future, tokio, deadlock, race condition"
user-invocable: false
---

# Concurrency

> **Layer 1: Language Mechanics**

## Core Question

**Is this CPU-bound or I/O-bound, and what's the sharing model?**

Before choosing concurrency primitives:
- What's the workload type?
- What data needs to be shared?
- What's the thread safety requirement?

---

## Error → Design Question

| Error | Don't Just Say | Ask Instead |
|-------|----------------|-------------|
| E0277 Send | "Add Send bound" | Should this type cross threads? |
| E0277 Sync | "Wrap in Mutex" | Is shared access really needed? |
| Future not Send | "Use spawn_local" | Is async the right choice? |
| Deadlock | "Reorder locks" | Is the locking design correct? |

---

## Thinking Prompt

Before adding concurrency:

1. **What's the workload?**
   - CPU-bound → threads (std::thread, rayon)
   - I/O-bound → async (tokio, async-std)
   - Mixed → hybrid approach

2. **What's the sharing model?**
   - No sharing → message passing (channels)
   - Immutable sharing → Arc<T>
   - Mutable sharing → Arc<Mutex<T>> or Arc<RwLock<T>>

3. **What are the Send/Sync requirements?**
   - Cross-thread ownership → Send
   - Cross-thread references → Sync
   - Single-thread async → spawn_local

---

## Trace Up ↑ (MANDATORY)

**CRITICAL**: Don't just fix the error. Trace UP to find domain constraints.

### Domain Detection Table

| Context Keywords | Load Domain Skill | Key Constraint |
|-----------------|-------------------|----------------|
| Web API, HTTP, axum, actix, handler | **rs-domain-web** | Handlers run on any thread |
| trading, payment, ledger | **rs-domain-fintech** | Audit + thread safety |
| gRPC, kubernetes, microservice | **rs-domain-cloud-native** | Distributed tracing |
| CLI, terminal, clap | **rs-domain-cli** | Usually single-thread OK |

### Example: Web API + Rc Error

```
"Rc cannot be sent between threads" in Web API context
    ↑ DETECT: "Web API" → Load rs-domain-web
    ↑ FIND: rs-domain-web says "Shared state must be thread-safe"
    ↑ FIND: rs-domain-web says "Rc in state" is Common Mistake
    ↓ DESIGN: Use Arc<T> with State extractor
    ↓ IMPL: axum::extract::State<Arc<AppConfig>>
```

### Generic Trace

```
"Send not satisfied for my type"
    ↑ Ask: What domain is this? Load rs-domain-* skill
    ↑ Ask: Does this type need to cross thread boundaries?
    ↑ Check: rs-domain-model (is the data model correct?)
```

| Situation | Trace To | Question |
|-----------|----------|----------|
| Send/Sync in Web | **rs-domain-web** | What's the state management pattern? |
| Send/Sync in CLI | **rs-domain-cli** | Is multi-thread really needed? |
| Mutex vs channels | rs-domain-model | Shared state or message passing? |
| Async vs threads | rs-performance | What's the workload profile? |

---

## Trace Down ↓

From design to implementation:

```
"Need parallelism for CPU work"
    ↓ Use: std::thread or rayon

"Need concurrency for I/O"
    ↓ Use: async/await with tokio

"Need to share immutable data across threads"
    ↓ Use: Arc<T>

"Need to share mutable data across threads"
    ↓ Use: Arc<Mutex<T>> or Arc<RwLock<T>>
    ↓ Or: channels for message passing

"Need simple atomic operations"
    ↓ Use: AtomicBool, AtomicUsize, etc.
```

---

## Send/Sync Markers

| Marker | Meaning | Example |
|--------|---------|---------|
| `Send` | Can transfer ownership between threads | Most types |
| `Sync` | Can share references between threads | `Arc<T>` |
| `!Send` | Must stay on one thread | `Rc<T>` |
| `!Sync` | No shared refs across threads | `RefCell<T>` |

## Quick Reference

| Pattern | Thread-Safe | Blocking | Use When |
|---------|-------------|----------|----------|
| `std::thread` | Yes | Yes | CPU-bound parallelism |
| `async/await` | Yes | No | I/O-bound concurrency |
| `Mutex<T>` | Yes | Yes | Shared mutable state |
| `RwLock<T>` | Yes | Yes | Read-heavy shared state |
| `mpsc::channel` | Yes | Optional | Message passing |
| `Arc<Mutex<T>>` | Yes | Yes | Shared mutable across threads |

## Decision Flowchart

```
What type of work?
├─ CPU-bound → std::thread or rayon
├─ I/O-bound → async/await
└─ Mixed → hybrid (spawn_blocking)

Need to share data?
├─ No → message passing (channels)
├─ Immutable → Arc<T>
└─ Mutable →
   ├─ Read-heavy → Arc<RwLock<T>>
   └─ Write-heavy → Arc<Mutex<T>>
   └─ Simple counter → AtomicUsize

Async context?
├─ Type is Send → tokio::spawn
├─ Type is !Send → spawn_local
└─ Blocking code → spawn_blocking
```

---

## Common Errors

| Error | Cause | Fix |
|-------|-------|-----|
| E0277 `Send` not satisfied | Non-Send in async | Use Arc or spawn_local |
| E0277 `Sync` not satisfied | Non-Sync shared | Wrap with Mutex |
| Deadlock | Lock ordering | Consistent lock order |
| `future is not Send` | Non-Send across await | Drop before await |
| `MutexGuard` across await | Guard held during suspend | Scope guard properly |

---

## Anti-Patterns

| Anti-Pattern | Why Bad | Better |
|--------------|---------|--------|
| Arc<Mutex<T>> everywhere | Contention, complexity | Message passing |
| thread::sleep in async | Blocks executor | tokio::time::sleep |
| Holding locks across await | Blocks other tasks | Scope locks tightly |
| Ignoring deadlock risk | Hard to debug | Lock ordering, try_lock |

---

## Async-Specific Patterns

### Avoid MutexGuard Across Await

```rust
// Bad: guard held across await
let guard = mutex.lock().await;
do_async().await;  // guard still held!

// Good: scope the lock
{
    let guard = mutex.lock().await;
    // use guard
}  // guard dropped
do_async().await;
```

### Non-Send Types in Async

```rust
// Rc is !Send, can't cross await in spawned task
// Option 1: use Arc instead
// Option 2: use spawn_local (single-thread runtime)
// Option 3: ensure Rc is dropped before .await
```

---

<!-- rules-index:start -->
## Rules (22)

Depth rules under `rules/` — open the one the question needs, not all of them.

**async-**

- [`async-async-fn-bounds`](rules/async-async-fn-bounds.md) — Use `AsyncFn`/`AsyncFnMut`/`AsyncFnOnce` bounds instead of `F: Fn() -> Fut, Fut: Future`
- [`async-bounded-channel`](rules/async-bounded-channel.md) — Use bounded channels to apply backpressure and prevent unbounded memory growth
- [`async-broadcast-pubsub`](rules/async-broadcast-pubsub.md) — Use `broadcast` channel for pub/sub where all subscribers receive all messages
- [`async-cancel-safety`](rules/async-cancel-safety.md) — Ensure futures used in `tokio::select!` branches are cancellation-safe
- [`async-cancellation-token`](rules/async-cancellation-token.md) — Use `CancellationToken` for graceful shutdown and task cancellation
- [`async-clone-before-await`](rules/async-clone-before-await.md) — Clone Arc/Rc data before await points to avoid holding references across suspension
- [`async-fn-in-trait`](rules/async-fn-in-trait.md) — Use native `async fn` in traits (stable 1.75) instead of the `async_trait` macro
- [`async-join-parallel`](rules/async-join-parallel.md) — Use `join!` or `try_join!` for concurrent independent futures
- [`async-joinset-structured`](rules/async-joinset-structured.md) — Use `JoinSet` for managing dynamic collections of spawned tasks
- [`async-mpsc-queue`](rules/async-mpsc-queue.md) — Use `mpsc` channels for async message queues between tasks
- [`async-no-lock-await`](rules/async-no-lock-await.md) — Never hold `Mutex`/`RwLock` across `.await`
- [`async-oneshot-response`](rules/async-oneshot-response.md) — Use `oneshot` channel for request-response patterns
- [`async-select-racing`](rules/async-select-racing.md) — Use `select!` to race futures and handle the first to complete
- [`async-spawn-blocking`](rules/async-spawn-blocking.md) — Use `spawn_blocking` for CPU-intensive work
- [`async-tokio-fs`](rules/async-tokio-fs.md) — Use `tokio::fs` instead of `std::fs` in async code
- [`async-tokio-runtime`](rules/async-tokio-runtime.md) — Configure Tokio runtime appropriately for your workload
- [`async-try-join`](rules/async-try-join.md) — Use `try_join!` for concurrent fallible operations with early return on error
- [`async-watch-latest`](rules/async-watch-latest.md) — Use `watch` channel for sharing the latest value with multiple observers

**conc-**

- [`conc-atomic-ordering`](rules/conc-atomic-ordering.md) — Use the weakest correct memory `Ordering` for every atomic operation
- [`conc-rayon-par-iter`](rules/conc-rayon-par-iter.md) — Use rayon's `par_iter()` for CPU-bound data parallelism
- [`conc-scoped-threads`](rules/conc-scoped-threads.md) — Use `std::thread::scope` to borrow stack data across threads
- [`conc-thread-local`](rules/conc-thread-local.md) — Prefer `thread_local!` with `Cell`/`RefCell` over `static mut`

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|------|-----|
| Smart pointer choice | rs-resource |
| Interior mutability | rs-mutability |
| Performance tuning | rs-performance |
| Domain concurrency needs | rs-domain-* |
| Actors, graceful shutdown, streams/backpressure, Pin | rs-async-patterns |
