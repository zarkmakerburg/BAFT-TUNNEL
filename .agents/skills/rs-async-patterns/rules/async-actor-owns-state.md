# async-actor-owns-state

> Let one task own mutable state and talk to it through a bounded mailbox
> **rs-async-patterns · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`Arc<Mutex<State>>` shared across async tasks invites lock-across-`.await` deadlocks and makes ordering implicit. A single owner serialises access by construction.

## Do

See `guides/actors.md`: enum `Msg`, `mpsc::channel(n)`, `oneshot` for replies, a `Clone` handle with typed methods. Keep slow work out of the actor loop (spawn it, send the result back).

## Don't

- hold a `std::sync::MutexGuard` across `.await`;
- let two actors wait on each other (A awaits B awaits A);
- expose the raw `Sender` — the handle's methods are the protocol.

## See Also

- [async-bound-every-queue](async-bound-every-queue.md)
- [rs-concurrency async-no-lock-await](../../rs-concurrency/rules/async-no-lock-await.md)
