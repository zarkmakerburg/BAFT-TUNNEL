# port-select-at-root

> Choose the backend in the composition root from configuration, never with `#[cfg]` in business code
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

The one place that knows which products exist is the composition root. Everything else receives a port. Then "move from Redis to X" is: write adapter X, add a config variant, flip it.

## Bad

```rust
#[cfg(feature = "redis")]  fn cache() -> RedisCache { /* … */ }
#[cfg(not(feature = "redis"))] fn cache() -> MemCache { /* … */ }   // business code decides the product
```

## Good

```rust
// wiring.rs
#[derive(Deserialize)] #[serde(tag = "kind", rename_all = "snake_case")]
pub enum CacheConfig { Memory { max: u64 }, Redis { url: String } }

pub fn build_cache(c: &CacheConfig) -> AnyCache {
    match c {
        CacheConfig::Memory { max } => AnyCache::Memory(MemCache::new(*max)),
        CacheConfig::Redis  { url } => AnyCache::Redis(RedisCache::connect(url)),
    }
}
```

## Choosing How to Select

- **Runtime config** (usual): enum dispatch or `Arc<dyn Port>`; validated at start-up, fail fast on bad config.
- **Cargo features**: use only to keep an adapter's heavy dependency out of builds that never need it; the `#[cfg]` stays inside `wiring.rs` and the adapter's own module.
- The config schema is a contract (rs-api-design `api-config-is-contract`): adding a backend adds a variant, never reshapes the others.

## See Also

- [port-async-and-dyn](port-async-and-dyn.md)
