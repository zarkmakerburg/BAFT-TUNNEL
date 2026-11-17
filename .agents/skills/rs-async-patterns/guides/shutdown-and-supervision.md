# Graceful shutdown and supervision

> **rs-async-patterns · guide** — verified pattern (tokio-util 0.7: `CancellationToken`, `TaskTracker`)

A service that cannot stop cleanly loses work, leaks connections and gets SIGKILLed by the orchestrator. Design shutdown first.

```rust
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub async fn run_workers(n: usize, stop: CancellationToken) {
    let tracker = TaskTracker::new();
    for id in 0..n {
        let stop = stop.clone();
        tracker.spawn(async move {
            loop {
                tokio::select! {
                    () = stop.cancelled() => break,          // finish the current unit, then exit
                    () = do_one_unit(id) => {}
                }
            }
        });
    }
    stop.cancelled().await;
    tracker.close();                                         // no new tasks
    let _ = tokio::time::timeout(Duration::from_secs(10), tracker.wait()).await;   // drain with a deadline
}
```

## The sequence

1. **Signal:** `tokio::signal::ctrl_c()` / SIGTERM handler cancels one root `CancellationToken`; children use `token.child_token()`.
2. **Stop intake:** close listeners, stop pulling from queues/streams — readiness probe goes unready first (rs-domain-cloud-native).
3. **Finish in-flight work:** tasks exit at their next safe point (loop top), not mid-write.
4. **Drain with a deadline:** `timeout(grace, tracker.wait())`; after the deadline log what is left and exit anyway.
5. **Flush:** telemetry, buffers, DB pools (`drop` order — rs-lifecycle), then exit with the right code.

## Cancel safety

`select!` drops the futures that lose. A branch is cancel-safe if dropping it between polls loses nothing (`mpsc::Receiver::recv`, `CancellationToken::cancelled`, `sleep`). Not cancel-safe: reads into your own buffer that you discard, multi-step writes. For those, hold the future outside the loop and `pin!` it, or make the step transactional (rs-concurrency `async-cancel-safety`).

## Supervision

| Need | Do |
|---|---|
| restart a crashed worker | loop around `JoinHandle`/`JoinSet`: on `Err(JoinError)` log with context and respawn with **backoff** and a restart budget (N restarts per window, then escalate) |
| a task must never be lost | keep its handle in a `JoinSet`/`TaskTracker` owned by a supervisor task; unhandled panics surface as `JoinError::is_panic()` |
| whole-process health | any critical task ending unexpectedly cancels the root token (fail fast) rather than leaving a zombie service |
| many actors with restart policies | use an actor framework's supervision (`ractor`, `actix`) — don't re-implement trees |

## Test

`#[tokio::test(start_paused = true)]` (tokio `test-util`) makes time virtual: assert that cancel produces exit within the deadline without sleeping for real. Add a test where a worker ignores the token and the deadline fires.
