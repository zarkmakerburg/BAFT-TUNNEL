# Pin, Unpin and hand-written futures

> **rs-async-patterns · guide** — verified examples (pin-project-lite 0.2)

## Why `Pin` exists

An `async fn` compiles to a state machine that may hold references into **its own fields** across `.await`. Moving it after it has started would invalidate those references, so a future must not move once polled. `Pin<P>` is the promise "the pointee will not move until dropped". Most types are `Unpin` (moving is harmless) and `Pin` does nothing for them; async blocks are `!Unpin`.

## Which pinning do I need?

| Situation | Tool |
|---|---|
| `.await`ing a future — the compiler pins it for you | nothing |
| store or return different futures behind one type | `Box::pin(fut)` → `Pin<Box<dyn Future<Output = T> + Send>>` |
| poll a future repeatedly in a loop / reuse it across `select!` iterations | `let fut = std::pin::pin!(fut);` (stack, no allocation) then `&mut fut` |
| a struct that wraps a future field and implements `Future` | `pin_project_lite::pin_project!` + `self.project()` — **do not** write the unsafe projection yourself |
| implement a `Stream`/`Future` combinator | `poll(self: Pin<&mut Self>, cx)`; register the waker; return `Pending` only after arranging a wake |

```rust
use std::{future::Future, pin::Pin, task::{Context, Poll}, time::{Duration, Instant}};

pin_project_lite::pin_project! {
    /// Measures how long the inner future takes across all its polls.
    pub struct Timed<F> { #[pin] inner: F, started: Option<Instant> }
}
impl<F> Timed<F> { pub fn new(inner: F) -> Self { Self { inner, started: None } } }

impl<F: Future> Future for Timed<F> {
    type Output = (F::Output, Duration);
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();                 // `inner` stays pinned, `started` is a plain &mut
        let start = *this.started.get_or_insert_with(Instant::now);
        match this.inner.poll(cx) {
            Poll::Ready(v) => Poll::Ready((v, start.elapsed())),
            Poll::Pending => Poll::Pending,        // inner already registered the waker
        }
    }
}

pub fn pick(fast: bool) -> Pin<Box<dyn Future<Output = u32> + Send>> {
    if fast { Box::pin(async { 1 }) } else { Box::pin(async { 2 }) }
}
// stack pinning in a function body:   let fut = std::pin::pin!(async { 3 }); fut.await
```

## Rules

- Never `unsafe { Pin::new_unchecked }` in application code; use `pin!`, `Box::pin`, `pin-project(-lite)`.
- A hand-written `poll` must never return `Pending` without a registered waker (the task would hang forever); never block inside `poll`.
- `Pin<&mut T>` where `T: Unpin` can be unwrapped with `Pin::get_mut`; for `!Unpin` you only get projections.
- Self-referential structs outside futures: don't. Use indices, `Rc`/`Arc`, or an arena; `ouroboros`/`self_cell` exist if unavoidable (rs-ownership).
- `dyn Future` needs `Pin<Box<…>>`; add `+ Send` when the future crosses `tokio::spawn`.
- `Send` errors from `tokio::spawn`: something non-`Send` (`Rc`, `MutexGuard`, `RefCell` ref) lives across an `.await` — shorten its scope (rs-concurrency `async-clone-before-await`).
