# async-graceful-shutdown

> Every long-running task takes a cancellation token; shutdown drains with a deadline
> **rs-async-patterns · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Services are stopped constantly (deploys, scaling, crashes of neighbours). Without a drain path they drop in-flight requests and corrupt half-written state; without a deadline they hang until SIGKILL.

## Do

```rust
let stop = CancellationToken::new();
let tracker = TaskTracker::new();
tracker.spawn(worker(stop.child_token()));
shutdown_signal().await;                 // SIGTERM / ctrl_c
stop.cancel();
tracker.close();
let _ = tokio::time::timeout(Duration::from_secs(10), tracker.wait()).await;
```

Order: unready → stop intake → finish in-flight → flush telemetry/pools → exit. Test with `start_paused = true` that cancel leads to exit within the deadline.

## See Also

- [async-bound-every-queue](async-bound-every-queue.md)
- [rs-concurrency async-cancellation-token](../../rs-concurrency/rules/async-cancellation-token.md)
