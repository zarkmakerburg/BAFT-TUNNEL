# port-async-and-dyn

> Async ports: declare `Send` futures; use enum dispatch for config-selected backends, `dyn` only for open sets
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Native `async fn` in traits is not usable as `dyn Trait`, and a plain `async fn` in a trait does not promise a `Send` future — which breaks `tokio::spawn`. Choosing the dispatch style up front avoids rewrites.

## Do

```rust
// Send futures, static dispatch (the default)
pub trait SessionCache: Send + Sync {
    fn get(&self, id: &SessionId) -> impl Future<Output = Result<Option<Session>, CacheError>> + Send;
}
```

| Situation | Dispatch |
|---|---|
| one backend per binary | generics (`Service<C: SessionCache>`) |
| backend picked from config, known set | **enum dispatch** (`AnyCache`) — zero cost, no extra crates, exhaustive `match` forces every new backend to be handled |
| open set / plugins / test doubles injected at runtime | `Arc<dyn Cache>` with `#[async_trait]` (heap-boxed futures) or `trait_variant::make` |
| decorators (retry, metrics, tracing) | wrap *any* `P: Port` in a struct that implements `Port` — works with all three styles |

## Pitfalls

- Never hold a `std::sync::Mutex` guard across `.await` inside an adapter (rs-concurrency).
- Pool/client handles are `Clone + Send + Sync`; build once in the root, pass by value/`Arc` (rs-lifecycle).
- Keep `Box<dyn Error + Send + Sync + 'static>` as the `source` type so errors cross threads.

## See Also

- [port-select-at-root](port-select-at-root.md)
- [rs-traits](../../rs-traits/SKILL.md)
