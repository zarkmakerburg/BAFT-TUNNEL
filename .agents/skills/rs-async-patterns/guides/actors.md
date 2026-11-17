# Actors — state owned by one task

> **rs-async-patterns · guide** — verified pattern (compiles on Rust 1.85+, tokio 1)

An actor is a task that owns some state and processes messages one at a time from a **bounded** mailbox. Others hold a cheap `Clone` handle. There is no lock, so there is no lock-across-`.await` bug and no data race on that state.

```rust
use tokio::sync::{mpsc, oneshot};

enum Msg {
    Incr { by: u64 },
    Get { reply: oneshot::Sender<u64> },
}

#[derive(Clone)]
pub struct CounterHandle { tx: mpsc::Sender<Msg> }

#[derive(Debug, thiserror::Error)]
pub enum ActorError { #[error("actor stopped")] Stopped }

impl CounterHandle {
    pub fn spawn(mailbox: usize) -> Self {
        let (tx, mut rx) = mpsc::channel(mailbox);         // bounded: back-pressure
        tokio::spawn(async move {
            let mut total = 0u64;                           // state lives only here
            while let Some(msg) = rx.recv().await {
                match msg {
                    Msg::Incr { by } => total += by,
                    Msg::Get { reply } => { let _ = reply.send(total); }   // the caller may be gone
                }
            }                                               // all handles dropped -> actor stops
        });
        Self { tx }
    }
    pub async fn incr(&self, by: u64) -> Result<(), ActorError> {
        self.tx.send(Msg::Incr { by }).await.map_err(|_| ActorError::Stopped)
    }
    pub async fn get(&self) -> Result<u64, ActorError> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(Msg::Get { reply }).await.map_err(|_| ActorError::Stopped)?;
        rx.await.map_err(|_| ActorError::Stopped)
    }
}
```

## Rules of the pattern

| Rule | Why |
|---|---|
| mailbox bounded; decide what a full mailbox means (wait / `try_send` + drop / error) | unbounded mailboxes hide a slow actor until memory runs out |
| messages are an `enum`; the handle exposes methods, not the channel | the protocol is typed and can change without touching callers |
| request/response = `oneshot` inside the message | no shared state, one reply, cancellation-friendly (`let _ = reply.send(..)`) |
| the actor never `await`s another actor that awaits it | A→B→A is a deadlock; break cycles with one-way messages or a third actor |
| a slow step inside the loop blocks the mailbox | `spawn` the slow work and send the result back as a message, or use `spawn_blocking` |
| panics kill the actor task | a supervisor (below) restarts it or the failure is surfaced — never silently continue |
| ordering is per sender | two senders have no mutual ordering guarantee |

## When to use a framework

Hand-rolled actors are right for one or a few state owners. For supervision trees, restart strategies, actor registries, remote actors or typed protocols use an existing crate rather than writing supervision yourself (rs-reuse-first): `ractor` (Erlang-style, supervision, optional distribution), `kameo`, `actix` (mature, own runtime integration). Verify current versions with `sdk-scout.sh`.

## Actor or lock?

| Use an actor | Use `Mutex`/`RwLock` (std) |
|---|---|
| state is touched by async code and operations take `.await` | tiny critical section, no `.await` inside |
| you need ordering, batching or rate-limiting of operations | read-mostly data (`RwLock`, `arc-swap`) |
| the state has a lifecycle (open/close, reconnect) | counters (`Atomic*`) |

## Test

`#[tokio::test]`; assert behaviour through the handle only. Drop all handles and assert the actor stops; fill the mailbox and assert the producer waits.
