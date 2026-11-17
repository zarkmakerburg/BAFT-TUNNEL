---
name: rs-ports-adapters
description: "CRITICAL: Use whenever code talks to anything that can change — a database, cache, queue, search/analytics engine, object store, identity provider, SDK, downstream service, LLM provider, clock or filesystem. Decouple it behind a port (trait) with swappable adapters so a product can be replaced without touching business code. Triggers: repository pattern, repository, adapter pattern, ports and adapters, hexagonal architecture, clean architecture, decouple, abstraction over database, swap database, replace Redis, replace ClickHouse, Postgres to, vendor lock-in, anti-corruption layer, gateway pattern, dependency inversion, dependency injection, composition root, in-memory fake, contract test, testcontainers, trait object, dyn Trait backend, storage trait, cache trait, message broker abstraction, licence change, replaceable"
user-invocable: false
---

# Ports & Adapters — keep every changeable product behind a boundary you own

> **Layer 2: Design Choices** · the answer to "what if we lose the licence / find something better / the vendor changes the API?"
>
> Tools: `${CLAUDE_PLUGIN_ROOT}/scripts/port-check.py` (finds vendor code outside adapters) · `templates/ports-adapters/` (compile-checked mini crate: port, memory adapter, vendor adapter, contract suite, composition root)

## Core Question

**If this product were replaced next year — Redis by something else, ClickHouse by another engine, one SDK by a newer one — how many files would change?**

The right answer is **one directory plus one line of configuration**. The repository pattern is the best-known case, but it is not special to databases: *anything whose replacement a licence, a price, a vendor decision, a scaling need or a better product could force* belongs behind a port. That is most of what a service touches.

---

## What goes behind a port

| Volatile thing | Port speaks in terms of | Typical adapters |
|---|---|---|
| Relational / document store | aggregates and use-case queries (`orders.find(id)`, `orders.open_for(customer)`) | Postgres, MySQL, SQLite, Mongo, in-memory |
| Analytics / time-series / columnar | intents (`daily_counts(range)`, `append(events)`) | ClickHouse, Druid, DuckDB, Parquet files, in-memory |
| Cache / session store / rate-limit counters | `get/put/invalidate` with TTL policy | Redis, Valkey, Memcached, `moka` in-process |
| Queue / event bus / streaming | `publish(event)`, `subscribe(topic)` with delivery semantics stated | Kafka, NATS, RabbitMQ, SQS, in-process channel |
| Search | `search(query) → hits` | Elasticsearch, OpenSearch, Meilisearch, tantivy |
| Object / file storage | `put/get/list(key)` | S3, GCS, Azure Blob, local disk (`object_store` is already this shape) |
| Identity / secrets / authorisation | `authenticate(token) → Principal`, `secret(name)` | OIDC providers, Vault, cloud KMS, Cedar/OPA |
| Protocol / agent SDK (inbound or outbound) | your use cases (`handle(tool_call)`), not the SDK's message types | `rmcp`, a gRPC stack, a REST client |
| Downstream services, third-party APIs | what you need from them, in your vocabulary | HTTP client, vendor SDK, stub |
| LLM / AI providers | `complete(prompt) → text` + your limits | several vendors, a local model, a recorded fake |
| Payments, email, SMS, push | `charge`, `send` | Stripe/Adyen, SMTP/SES, test sink |
| Time, randomness, ids, filesystem, environment | `now()`, `new_id()` | system, fixed (tests) |

**Not worth a port:** `std`, `serde`, `uuid`-style pure libraries, your own pure code, and a dependency you would never replace *and* never need to fake. A port needs either a plausible second implementation or a test fake. See `rules/port-yagni.md`.

## Vocabulary (use these words in code and Design Contracts)

| Term | Meaning here |
|---|---|
| **Port** | a trait in the feature module, written in business language; the only thing business code depends on |
| **Adapter** | an implementation of a port for one product, living in `adapters/<product>/` |
| **Repository** | a port for persisting and loading aggregates |
| **Gateway** | a port for an external service or API |
| **Anti-corruption layer** | the mapping inside an adapter that translates the vendor's types, errors and quirks to yours |
| **Fake** | an in-memory adapter used in tests and as the reference semantics |
| **Composition root** | the one place (`main`/`wiring.rs`) that reads configuration and picks the adapters |

## The six rules that make it real

1. **The domain never imports a vendor crate.** No `sqlx::`, `redis::`, `clickhouse::`, `rmcp::` outside `adapters/` and the composition root. Enforce with crate boundaries (core crate has no vendor deps) or `${CLAUDE_PLUGIN_ROOT}/scripts/port-check.py`.
2. **Design the port from the use case, not from the product.** Methods express intent and use domain types. `fn query(&self, sql: &str)` is the product leaking through the trait. (`port-design-by-need`)
3. **Errors are yours.** Adapters classify vendor errors into a small port error (`Unavailable`, `Timeout`, `Rejected`, `Conflict`, `NotFound`…) with the vendor error kept as `source`. Retry policy then works for every backend. (`port-map-errors`, rs-domain-error)
4. **Row/wire types stay in the adapter.** No vendor derives (`FromRow`, `Row`, `Queryable`) on domain types; map `From`/`TryFrom` at the boundary. (`port-row-types`)
5. **One contract suite, every adapter passes it.** The in-memory adapter defines the semantics; real adapters run the same generic test (testcontainers or a feature flag). That is how you *know* a replacement is correct. (`port-contract-tests`)
6. **Pick the backend in one place, by configuration.** Enum dispatch, `Arc<dyn Port>` or generics — chosen at the composition root, never `#[cfg]` inside business code. (`port-select-at-root`)

## Rationalizations to reject

| Excuse | Reality |
|---|---|
| "We'll never change the database." | Licences, prices and better products change it for you — the user's own Redis/ClickHouse example. The port is one trait and one directory. |
| "A trait for everything is over-engineering." | Agreed — only at volatility boundaries (`port-yagni`). A port needs a second implementation or a test fake. |
| "`Repository<T>` with CRUD is decoupled." | It is the table API in disguise (`port-no-generic-repository`). |
| "I'll just put `#[cfg(feature = "redis")]` in the service." | The backend choice belongs in the composition root (`port-select-at-root`). |
| "Mocking the SDK client is enough." | That tests the mock. Fake adapter + contract suite against the real product. |

## Layout (rs-structure applies: group by feature)

```
src/
  audit/                 ← feature (business language)
    mod.rs  model.rs  ports.rs  service.rs      ← port lives with the feature that needs it
  adapters/              ← one directory per PRODUCT, implementing the ports it can serve
    memory/    audit.rs  sessions.rs
    clickhouse/ audit.rs  client.rs             ← shared connection/config for that product
    redis/     sessions.rs
  testing/               ← contract suites, generic over the port
  wiring.rs              ← composition root: Config → adapters
```
Large systems: separate crates — `core` (domain + ports, **no vendor dependencies**), `adapter-<product>`, `app` (wiring + binaries). The compiler then enforces rule 1. Full examples in `templates/ports-adapters/`.

## Dispatch choices (details: `rules/port-async-and-dyn.md`)

| Need | Use |
|---|---|
| backend fixed at compile time | generic service `Service<S: Port>`; zero cost |
| backend chosen from config at start-up | **enum dispatch** (`AnyStore::{Memory, ClickHouse}`) — no `dyn`, no `async_trait`; the template shows it |
| backends are plugins / unknown set | `Arc<dyn Port>` — async methods need `#[async_trait]`, `trait_variant` or boxed futures |
| several products behind one port at once | a router/fan-out adapter that itself implements the port (write-through cache, dual-write migration) |

## When you add or review a feature

| Question | If "no" |
|---|---|
| Can I run the business tests with only the in-memory adapter? | the port is too fat or leaky |
| Does `port-check.py` report zero findings? | move the vendor code into an adapter |
| Is there a contract suite and does every adapter run it? | add it before the second adapter exists |
| Could I swap the product by adding a directory + a config value? | the swap drill (`port-swap-drill`) fails — find what leaked |
| Are vendor errors mapped before they cross the port? | callers will `match` on vendor types sooner or later |
| Does the Design Contract name the port and the adapters? | rs-design-contract: *Patterns:* "Port `AuditStore`; adapters in `adapters/`" |

## Trace

| Direction | Go to |
|---|---|
| ↑ Up | rs-domain-model (what is an aggregate, what does the repository return) · rs-reuse-first (which SDK sits behind the adapter) · rs-domain-* (latency/consistency constraints that shape the port) |
| ↓ Down | rs-traits (async fn in traits, object safety) · rs-error-handling (error mapping) · rs-concurrency (never hold a lock across `.await` in an adapter) · rs-lifecycle (pools, shutdown) |
| → Delivery | rs-testing (contract suite, fakes, testcontainers) · rs-structure (adapters layout) · rs-review (port-check in the machine pass) · rs-cleanup (containers started by integration tests) |

<!-- rules-index:start -->
## Rules (12)

Depth rules under `rules/` — open the one the question needs, not all of them.

**port-**

- [`port-async-and-dyn`](rules/port-async-and-dyn.md) — Async ports: declare `Send` futures; use enum dispatch for config-selected backends, `dyn` only for open sets
- [`port-contract-tests`](rules/port-contract-tests.md) — One generic test suite per port, run by every adapter including the in-memory fake
- [`port-design-by-need`](rules/port-design-by-need.md) — Shape the port from the use case in business words, not from the product's API
- [`port-map-errors`](rules/port-map-errors.md) — Classify vendor errors at the boundary; keep the vendor error as `source`, never as a variant
- [`port-no-generic-repository`](rules/port-no-generic-repository.md) — Don't write `Repository<T>` with `find/save/delete` for everything
- [`port-no-vendor-in-domain`](rules/port-no-vendor-in-domain.md) — Business code never names a vendor crate; dependencies point inward
- [`port-row-types`](rules/port-row-types.md) — Give each adapter its own row/DTO type; domain types carry no vendor derives
- [`port-select-at-root`](rules/port-select-at-root.md) — Choose the backend in the composition root from configuration, never with `#[cfg]` in business code
- [`port-swap-drill`](rules/port-swap-drill.md) — Rehearse replacing a product on paper (and in a spike) before you need to
- [`port-uow-transactions`](rules/port-uow-transactions.md) — Keep transactions inside the port: one aggregate per call, or an explicit unit-of-work closure
- [`port-volatility-inventory`](rules/port-volatility-inventory.md) — List the changeable products at design time and decide the port for each
- [`port-yagni`](rules/port-yagni.md) — A port needs a second implementation or a test fake — otherwise don't add one

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|---|---|
| DDD vocabulary — entity, aggregate, repository | rs-domain-model |
| Picking the SDK that sits behind an adapter | rs-reuse-first |
| Where adapters and ports live | rs-structure |
| Retry, backoff, circuit breaker around an adapter | rs-domain-error |
| Test doubles and integration tests | rs-testing |
