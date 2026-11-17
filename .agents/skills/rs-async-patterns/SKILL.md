---
name: rs-async-patterns
description: "Use for async architecture beyond the basics — actors and mailboxes, supervision, graceful shutdown, streams and backpressure, cancellation, Pin/Unpin and hand-written futures, spawning with 'static, bounded concurrency. Triggers: actor, actor model, mailbox, supervision tree, supervisor, ractor, kameo, actix, message passing, graceful shutdown, CancellationToken, TaskTracker, drain, SIGTERM, Stream, StreamExt, backpressure, bounded channel, buffer_unordered, select!, cancel safety, timeout, Pin, Unpin, pin!, Box::pin, pin-project, self-referential future, impl Future, poll, Waker, tokio::spawn 'static, JoinSet, semaphore, bounded concurrency, structured concurrency, async trait object"
user-invocable: false
---

# Async Patterns — architecture for long-running async programs

> **Layer 1/2: Mechanics + Design** · what rs-concurrency's rules assume you already decided
>
> Depth: `guides/actors.md` · `guides/shutdown-and-supervision.md` · `guides/streams-and-backpressure.md` · `guides/pin-and-futures.md` · basic rules live in rs-concurrency (`async-*`)

## Core Question

**Who owns this state, who may wait for whom, and what happens when the program is told to stop?**

Async bugs are rarely syntax; they are ownership of state across tasks, unbounded queues, work that is cancelled half-way, and shutdowns that never finish. Decide these four before writing the first `spawn`.

---

## Thinking Prompt

1. **State:** shared with a lock, or *owned by one task* that others talk to? (Owned-by-one-task is the actor pattern — no `Mutex`, no lock-across-`.await`.)
2. **Queues:** is every channel/stream/buffer **bounded**? An unbounded queue is an out-of-memory bug waiting for a slow consumer.
3. **Cancellation:** if this future is dropped at any `.await`, is the state still consistent? (cancel safety — `select!` drops the losers)
4. **Shutdown:** how does each task learn to stop, how long may it take, who waits for it?
5. **Lifetimes:** does spawned work need borrowed data? (`tokio::spawn` needs `'static`: move/`Arc`, or use scoped/structured concurrency.)

## Decision table

| Situation | Use | Detail |
|---|---|---|
| mutable state touched from many tasks | **actor**: one task owns it, handles messages from a bounded `mpsc`; replies via `oneshot` | `guides/actors.md` |
| need restart-on-crash, many actors, remote actors | an actor framework (`ractor`, `kameo`, `actix`) — don't build supervision trees by hand | `guides/actors.md` |
| process must exit cleanly on SIGTERM | one `CancellationToken` + `TaskTracker`, drain with a deadline | `guides/shutdown-and-supervision.md` |
| a producer faster than its consumer | bounded channel / `Stream` with `buffer_unordered(n)` / semaphore; let `send().await` push back | `guides/streams-and-backpressure.md` |
| first of several futures wins | `tokio::select!` — only with cancel-safe branches | rs-concurrency `async-select-racing`, `async-cancel-safety` |
| N things in parallel, fail fast | `JoinSet` / `try_join!` | rs-concurrency `async-joinset-structured`, `async-try-join` |
| limit parallelism | `Semaphore`, `buffer_unordered(n)`, never `spawn` in an unbounded loop | `guides/streams-and-backpressure.md` |
| wrap/instrument a future, implement a combinator | `pin_project_lite`, `Pin<&mut Self>`; `Box::pin` for erasure; `std::pin::pin!` for the stack | `guides/pin-and-futures.md` |
| blocking or CPU-bound call inside async | `spawn_blocking` / rayon, never `std::thread::sleep` or heavy loops in a task | rs-concurrency `async-spawn-blocking` |
| `dyn` async trait | `#[async_trait]`/`trait_variant`, or enum dispatch | rs-ports-adapters `port-async-and-dyn` |

## The five rules

1. **Bound every queue.** `mpsc::channel(n)`, not `unbounded_channel()`; decide per queue what happens when full (wait, drop oldest, fail).
2. **One owner per piece of state.** Prefer a task that owns it over `Arc<Mutex<…>>`; if you must lock, never hold the guard across `.await`.
3. **Every long-running task takes a cancellation signal** and finishes its current unit of work before exiting; the program waits for them with a deadline.
4. **`select!` only with cancel-safe branches**, or loop with the future pinned outside the `select!` so it is not dropped (`pin!`).
5. **No fire-and-forget `tokio::spawn` without a handle** you either await, track (`TaskTracker`/`JoinSet`) or deliberately detach with a comment saying why.

## Trace

| Direction | Go to |
|---|---|
| ↑ Up | rs-domain-cloud-native (shutdown on SIGTERM, health), rs-domain-iot/-web (what the loop serves), rs-domain-distributed (timeouts, retries across the network) |
| ↓ Down | rs-concurrency (Send/Sync, channels, rules), rs-mutability (locks), rs-lifecycle (Drop, pools), rs-traits (async traits) |
| → Delivery | rs-testing (`#[tokio::test(start_paused = true)]` for time, loom/shuttle for races), rs-observability (task and span context), rs-cleanup (no leaked processes) |

<!-- rules-index:start -->
## Rules (4)

Depth rules under `rules/` — open the one the question needs, not all of them.

**async-**

- [`async-actor-owns-state`](rules/async-actor-owns-state.md) — Let one task own mutable state and talk to it through a bounded mailbox
- [`async-bound-every-queue`](rules/async-bound-every-queue.md) — Bound every channel, buffer and in-flight set; decide what "full" means
- [`async-graceful-shutdown`](rules/async-graceful-shutdown.md) — Every long-running task takes a cancellation token; shutdown drains with a deadline
- [`async-no-bare-spawn`](rules/async-no-bare-spawn.md) — Don't fire-and-forget `tokio::spawn`; track the handle or say why it is detached

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|---|---|
| Send/Sync, channels, `async-*` rules | rs-concurrency |
| Retry, circuit breaker | rs-domain-error |
| Distributed timeouts and idempotency | rs-domain-distributed |
| Ownership of data across tasks | rs-ownership, rs-resource |
