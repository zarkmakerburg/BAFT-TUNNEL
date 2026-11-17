# Standards registry — starting points, not verdicts

> **rs-reuse-first · registry** — verified against crates.io on 2026-11-11. Versions and activity drift:
> always re-check with `scripts/sdk-scout.sh "<term>" --github` before you commit to a row.
> "Official" means published from the repository of the organisation that owns the standard or the product.

How to use: find the standard in the left column, take the crate as the **candidate**, run the scout, read its README/CHANGELOG
for the spec version and the roles (client/server) you need, then record the decision (`templates/decision-record.md`).
Not listed? Run the scout anyway — a missing row is not a licence to hand-roll.

## Agent and tool protocols

| Standard | Candidate | Status | Repository | Notes |
|---|---|---|---|---|
| **Model Context Protocol** (client + server) | `rmcp` 3.9 | **official** | modelcontextprotocol/rust-sdk | tokio; features `server`, `client`, `macros` (`#[tool]`), `transport-io` (stdio), `transport-streamable-http-server/-client`, `auth` (OAuth 2.0), `elicitation`; implements spec `2026-07-28`, compatible with `2025-11-25` and earlier; read the 3.x migration guide. Never hand-write the JSON-RPC envelope, capability negotiation or transports |
| JSON-RPC 2.0 (generic) | `jsonrpsee` | de-facto | paritytech/jsonrpsee | HTTP/WS client+server; use only when the protocol is *not* MCP |
| Agent2Agent (A2A) | scout it | young ecosystem | — | `a2a-rs` exists but small; check the A2A org's own SDK first |
| LLM provider clients | `genai`, `rig-core`, `async-openai` | community | — | no single winner: put the provider behind a port (rs-ports-adapters) |

## Web, RPC and messaging

| Standard | Candidate | Status | Notes |
|---|---|---|---|
| HTTP server | `axum` 0.8 (on `hyper` 1.x, `tower`) | de-facto (tokio-rs) | `actix-web` is the other mature choice |
| HTTP client | `reqwest` 0.13 | de-facto | `hyper-util` when you need the raw layer |
| gRPC + protobuf | `tonic` 0.14 + `prost` | de-facto (grpc/grpc-rust) | `tonic-build`/`prost-build` generate the code — don't write messages by hand |
| WebSocket | `tokio-tungstenite`, `axum::extract::ws` | de-facto | |
| Server-Sent Events | `axum::response::sse` (server); `eventsource-stream` / `reqwest-eventsource` (client, check staleness) | de-facto | the MCP SDK handles SSE/streamable HTTP for you |
| GraphQL | `async-graphql` | de-facto | `juniper` alternative |
| OpenAPI | `utoipa` 6 (+ `utoipa-axum`), `aide` | de-facto | generate the spec from code, don't hand-edit YAML |
| Kafka | `rdkafka` | de-facto (librdkafka binding) | pure-Rust `rskafka` if you cannot link C |
| AMQP / RabbitMQ | `lapin` | de-facto | |
| NATS | `async-nats` | **official** (nats-io) | |
| MQTT | `rumqttc` (client), `rumqttd` (broker) | de-facto | rs-domain-iot |
| Retry / backoff | `backon`, `tower::retry` | de-facto | rs-domain-error — never a hand-written loop |
| Rate limiting | `governor`, `tower::limit` | de-facto | |
| Connection pools | `deadpool`, `bb8`; `sqlx`/`redis`/`mongodb` pool built in | de-facto | |
| In-process cache | `moka` | de-facto | rs-ports-adapters: behind a `Cache` port |

## Identity and security

| Standard | Candidate | Status | Notes |
|---|---|---|---|
| OAuth 2.0 | `oauth2` 5 | de-facto | for MCP, prefer `rmcp`'s `auth` feature |
| OpenID Connect | `openidconnect` | de-facto (same author as `oauth2`) | |
| JWT | `jsonwebtoken` 11 | de-facto | `josekit` for full JOSE (JWE/JWK) |
| TLS | `rustls` (+ `tokio-rustls`, `rustls-pemfile`) | de-facto | avoid hand-rolled certificate handling |
| Password hashing | `argon2` (RustCrypto) | de-facto | never roll your own KDF |
| Authorization policy | `cedar-policy` (AWS/Cedar), `regorus` (OPA/Rego, Microsoft) | official | policy engines, not `if` ladders |
| Crypto primitives | RustCrypto crates, `ring`, `aws-lc-rs` | de-facto | |

## Data stores (always behind a port — rs-ports-adapters)

| Product | Candidate | Status | Notes |
|---|---|---|---|
| PostgreSQL / MySQL / SQLite | `sqlx` 0.9 (async, compile-time-checked), `tokio-postgres`, `diesel` | de-facto | |
| Redis / Valkey | `redis` (redis-rs) 1.x, `fred` | **official-org** (redis-rs) / community | `fred` has not released since early 2025 at the time of writing — check |
| ClickHouse | `clickhouse` 0.15 | **official** (ClickHouse/clickhouse-rs) | |
| MongoDB | `mongodb` 3.x | **official** | |
| Elasticsearch / OpenSearch | `elasticsearch` (alpha at time of writing), `opensearch` | **official** | |
| Object storage (S3, GCS, Azure) | `object_store` (Apache), `aws-sdk-s3` | official / de-facto | `object_store` is already a port-shaped abstraction over clouds |
| Arrow / Parquet / query engine | `arrow`, `parquet`, `datafusion` | official (Apache) | |

## Cloud, platform and observability

| Standard | Candidate | Status | Notes |
|---|---|---|---|
| AWS | `aws-sdk-*` (+ `aws-config`) | **official** (awslabs) | one crate per service |
| Kubernetes | `kube` 5 + `k8s-openapi` | de-facto (kube-rs, CNCF sandbox) | no hand-written REST calls to the API server |
| Docker Engine API | `bollard` | de-facto | integration tests: `testcontainers` |
| OpenTelemetry | `opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry` | **official** | rs-observability |
| Prometheus | `metrics` + `metrics-exporter-prometheus`, or `opentelemetry` | de-facto | |
| WebAssembly runtime | `wasmtime` | **official** (Bytecode Alliance) | |
| proxy-wasm filters | `proxy-wasm` 0.2 | **official** | Flex PDK wraps it; rs-domain-gateway |
| DNS | `hickory-resolver` | de-facto | formerly trust-dns |
| SMTP | `lettre` | de-facto | |

## Data formats and small standards

| Need | Candidate | Notes |
|---|---|---|
| JSON / YAML / TOML | `serde_json`; `serde_yaml` is **deprecated** (archived) — use a maintained fork (e.g. `serde_yaml_ng`) or `yaml-rust2`-based crates after checking; `toml` | |
| Decimal money | `rust_decimal` | rs-domain-fintech |
| Date and time | `jiff` (modern), `chrono`, `time` | pick one per project |
| UUID, base64, hex, URL, semver, regex | `uuid`, `base64`, `hex`, `url`, `semver`, `regex` | never hand-roll |
| Config layering | `figment`, `config` | rs-domain-cli |
| CLI | `clap` | rs-domain-cli |
| Errors | `thiserror` (libraries), `anyhow` (binaries) | rs-error-handling |

## Rows to add when you learn a new one

Add a row with the date you verified it, the repository, and one line on what to watch (spec version, MSRV, maintenance). A row without a verification date is a rumour.
