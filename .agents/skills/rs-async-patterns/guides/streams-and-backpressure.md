# Streams, bounded concurrency and backpressure

> **rs-async-patterns · guide**

**Backpressure** = the slow part slows the fast part, instead of memory growing. In async Rust it comes from `await` on a *bounded* thing.

| Mechanism | Backpressure behaviour |
|---|---|
| `mpsc::channel(n)` `send().await` | producer waits when full |
| `Semaphore::acquire().await` | callers wait for a permit |
| `stream.buffer_unordered(n)` | at most `n` futures in flight; upstream is polled only when there is room |
| `for_each_concurrent(n, …)` | same, with a sink |
| TCP/HTTP/gRPC flow control | `write().await` pending when the peer is slow |
| `unbounded_channel`, `Vec` buffer, `spawn` per item | **none** — memory grows until OOM |

## Patterns

```rust
use futures::{stream, StreamExt};

// bounded fan-out: 16 requests at a time, results as they finish
let results: Vec<_> = stream::iter(urls)
    .map(|u| async move { fetch(u).await })
    .buffer_unordered(16)
    .collect()
    .await;
```
- **Ordered results:** `buffered(n)` (preserves order, head-of-line blocking) vs `buffer_unordered(n)`.
- **Batching:** `tokio_stream::StreamExt::chunks_timeout(n, dur)` (needs `tokio-stream` `time`) — flush by size or time; batch inserts (rs-domain-data analytics stores).
- **Rate limit:** `governor` or `tower::limit` in front of the call; not `sleep` in the loop.
- **Fan-in from many sources:** `tokio_stream::StreamMap`/`select_all`; each source bounded.
- **Slow consumer policy** — decide explicitly: `send().await` (wait), `try_send` (drop newest), a ring buffer (drop oldest), or fail the connection. `broadcast` lags receivers: handle `RecvError::Lagged`.
- **Writing a Stream:** prefer `async-stream` (`stream!`) or `futures::stream::unfold` over a hand-written `poll_next`; implement `poll_next` only for combinators (`guides/pin-and-futures.md`).
- **Cancellation:** a dropped stream stops the work it was driving; ensure side effects are idempotent or transactional.

## Review questions

Where is the slowest component? What happens to the producer when it stalls for a minute? How many items can be in memory at once — is that number in the code?
