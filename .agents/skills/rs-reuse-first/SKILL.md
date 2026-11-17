---
name: rs-reuse-first
description: "CRITICAL: Use BEFORE writing any code that implements a protocol, standard, specification or vendor/product client — search the market for the official or de-facto SDK first, never draft it from scratch. Triggers: implement a protocol, from scratch, write our own, custom implementation, spec, RFC, standard, SDK, official SDK, client library, build a server, build a client, MCP server, MCP client, Model Context Protocol, rmcp, JSON-RPC, gRPC, OAuth, OIDC, JWT, SAML, GraphQL, OpenAPI, WebSocket, SSE, Kafka, AMQP, MQTT, NATS, Redis, ClickHouse, Postgres, S3, Kubernetes, OpenTelemetry, which crate, is there a crate for, reinvent the wheel, build vs buy, hand-rolled parser, parse the wire format, don't reinvent"
user-invocable: false
---

# Reuse First — standards and SDKs before custom code

> **Layer 2: Design Choices** · the gate that runs *before* the first line of an implementation
>
> Tools: `${CLAUDE_SKILL_DIR}/scripts/sdk-scout.sh` (market search) · `registry/standards.md` (starting points) · `templates/decision-record.md` · rs-review `risk-scan.py` (finds hand-rolled standards in existing code)

## Core Question

**Has someone already built, and keeps maintaining, the thing I am about to write?**

A hand-drafted implementation of a protocol or product API will pass the happy path and miss the rest: version negotiation, error codes, transports, auth, edge cases, spec updates. The maintained SDK has already met those cases and keeps meeting them as the spec moves. Drafting it yourself is the most expensive shortcut in a project: it looks fast, then it is rewritten — usually after the design has grown around it.

---

## The gate (non-negotiable)

Run it whenever the task involves **a named standard, protocol, file format, wire format, or a vendor/product API** (MCP, JSON-RPC, gRPC, OAuth/OIDC, JWT, GraphQL, SSE, AMQP, Kafka, Redis, ClickHouse, S3, Kubernetes, OpenTelemetry, …) — even when the user did not ask for a library.

| Step | Do | Output |
|---|---|---|
| 1. Name it | the standard, the **roles** you need (client / server / both), the **spec version**, the runtime constraints (tokio? wasm? `no_std`?) | one line |
| 2. Look in-tree | `Cargo.toml` of the workspace and `cargo tree -d`; is it already a dependency or a transitive one? | reuse what is there |
| 3. Look at the registry | `registry/standards.md` — the curated starting point (dated; verify) | candidate list |
| 4. Search the market | `${CLAUDE_SKILL_DIR}/scripts/sdk-scout.sh "<standard>" [<vendor>] --github` — crates.io by downloads + GitHub by stars; flags `OFFICIAL?`, `STALE`, `IN-TREE`. If it returns nothing useful, search the **spec organisation's GitHub** (`github.com/<spec-org>/rust-sdk`) and lib.rs/docs.rs by hand | evidence, not memory |
| 5. Evaluate the top candidates | scorecard in `rules/reuse-evaluate-candidate.md`: official? spec version? roles/transports? health? licence? MSRV? | pick + runner-up |
| 6. Spike (optional, cheap) | a scratch crate in the scratchpad that calls the SDK once; **delete it afterwards** (rs-cleanup) | proof it fits |
| 7. Decide and record | `templates/decision-record.md` → `<docs>/decisions/NNNN-<topic>.md` (or the Design Contract's *Patterns* line for small cases) | the decision is auditable |
| 8. Isolate | wrap the SDK behind a port you own — rs-ports-adapters — so it can be replaced | SDK types never reach the domain |
| 9. Say it | put "Reuse report" (below) in your answer to the user | they can overrule it early |

```markdown
### Reuse report
- Need: <standard / roles / spec version / constraints>
- Searched: crates.io + GitHub (`sdk-scout`), registry, in-tree
- Chosen: <crate> <version> (<official | de-facto>) — why: <one line>
- Runner-up: <crate> — why not: <one line>
- Gaps: <what the SDK does not cover> → <contribute | wrap | hand-roll with a decision record>
```

Skipping the gate is only acceptable for **trivial formats** (a three-field config struct) — never for anything with a version number, a handshake or an error taxonomy.

## The reuse ladder

| Rung | Use when | Notes |
|---|---|---|
| 0. `std` / what is already in the tree | it covers the need | cheapest dependency is the one you have |
| 1. **Official SDK** of the spec owner / vendor | one exists | e.g. `rmcp` for MCP (`modelcontextprotocol/rust-sdk`), `clickhouse` (ClickHouse org), `mongodb`, `kube`, `aws-sdk-*`, `cedar-policy` |
| 2. De-facto community crate | no official SDK; one clearly dominates (downloads, recent releases, used by other well-known crates) | `redis`, `sqlx`, `tonic`, `jsonwebtoken` |
| 3. Compose lower-level standard crates | no SDK, but the protocol rides on one you can build with (`hyper`, `serde`, `tokio-util` codecs) | still no hand-written parsing of the wire format |
| 4. **Hand-roll** | nothing exists, or a hard constraint excludes every candidate (wasm sandbox without tokio, `no_std`, a hot path the SDK cannot meet — *measured*) | needs a decision record, spec-derived conformance tests, and a plan to switch when an SDK appears |

Going down a rung needs a reason you can write in one sentence. "The SDK looked heavy" and "I know the protocol" are not reasons.

## Smells that you are reinventing (stop and run the gate)

| You are writing… | Probably exists as |
|---|---|
| structs with `jsonrpc: "2.0"`, `id`, `method`, `params` and a dispatcher | an MCP/JSON-RPC SDK (`rmcp`, `jsonrpsee`) |
| a `Content-Length` / `data:` / `event:` frame parser | the SDK's transport, `axum::response::sse`, `eventsource-stream` |
| `split('.')` + base64 on a token | `jsonwebtoken`, `josekit` |
| OAuth redirect/token-exchange code by hand | `oauth2`, `openidconnect`, the SDK's `auth` feature |
| `reqwest` calls shaped like a vendor REST API (AWS, Kubernetes, ClickHouse HTTP) | the vendor's SDK (`aws-sdk-*`, `kube`, `clickhouse`) |
| retry/backoff loops, token buckets, circuit breakers | `backon`, `tower`, `governor` (rs-domain-error) |
| your own connection pool / cache with TTL | `deadpool`/`bb8`, `moka` |
| manual protobuf/varint encoding | `prost` + `tonic` |
| hand-written UUID, base64, hex, URL, semver, time-zone handling | `uuid`, `base64`, `hex`, `url`, `semver`, `jiff`/`chrono` |

## Rationalizations to reject

| Excuse | Reality |
|---|---|
| "I know the protocol, I'll write the envelope myself." | You know the happy path. The SDK has the version negotiation, error codes, transports, auth and the next spec revision. |
| "The SDK looks heavy." | Disable features (`default-features = false`) and measure. "Looks heavy" is not a reason; a measured constraint is. |
| "Quicker to draft it now." | The user redoes it on the SDK later — it already happened once with an MCP server. Search first (5 minutes). |
| "It's just a small subset." | Subsets grow. Without spec-derived fixtures and an exit plan, a subset is a fork you maintain. |
| "No crate exists." | Say how you searched (`sdk-scout`, spec org, lib.rs) and when. "I didn't find one" without evidence is not a finding. |

## Using the SDK well (so you don't end up rewriting it anyway)

- Read the SDK's README, `examples/` and CHANGELOG first; start from the nearest example, don't re-derive the wiring.
- Enable only the features you need (`default-features = false`) and pin a compatible range; record the spec version it implements.
- **Do not fork-and-patch silently.** Missing feature → upstream issue/PR or an extension trait in your crate; note it in the decision record.
- Keep SDK types at the edge: translate to your domain types in an adapter (rs-ports-adapters) so a major SDK bump or a replacement touches one module.
- Test against the real protocol behaviour (the SDK's own client against your server, or a conformance suite) — rs-testing.

## Hard constraints that exclude an SDK

| Constraint | Do |
|---|---|
| wasm sandbox / proxy-wasm filter (no tokio, no threads) | reuse the *types* or codec crate if it compiles for the target; otherwise hand-roll only the **subset the filter inspects**, derive fixtures from the spec, and record it (rs-domain-gateway) |
| `no_std` / embedded | `no_std`-capable crates first (`heapless`, `postcard`, vendor HALs), then a minimal hand-roll with a decision record (rs-domain-embedded) |
| licence or security policy forbids the SDK | `cargo deny` result in the record; pick the runner-up before hand-rolling |
| measured performance gap | the benchmark and the SDK's numbers go in the record (rs-performance) |

## Trace

| Direction | Go to |
|---|---|
| ↑ Up | rs-domain-* — which constraints (latency, deployment, target) decide between candidates? |
| ↓ Down | rs-ports-adapters (wrap it) · rs-ecosystem (features, workspace deps) · rs-traits / rs-error-handling (integrating its types and errors) · rs-testing (conformance) |
| → Delivery | rs-release (`cargo deny`, MSRV, semver of *your* API when the SDK is public in it) · rs-review (reinvention scan) |

<!-- rules-index:start -->
## Rules (7)

Depth rules under `rules/` — open the one the question needs, not all of them.

**reuse-**

- [`reuse-evaluate-candidate`](rules/reuse-evaluate-candidate.md) — Score candidates on spec fit, roles, health and licence — not on first impressions
- [`reuse-handroll-last-resort`](rules/reuse-handroll-last-resort.md) — Hand-roll only with a written reason, spec-derived tests and an exit plan
- [`reuse-migrate-handrolled`](rules/reuse-migrate-handrolled.md) — Replace an existing hand-rolled standard with the SDK in compiling, tested steps
- [`reuse-official-sdk-first`](rules/reuse-official-sdk-first.md) — The SDK published by the owner of the spec or product beats every alternative
- [`reuse-search-before-write`](rules/reuse-search-before-write.md) — Search crates.io, GitHub and the spec's own organisation before writing any protocol or vendor client
- [`reuse-spec-version-check`](rules/reuse-spec-version-check.md) — Record which revision of the spec the chosen SDK implements and re-check on every major bump
- [`reuse-wrap-dont-leak`](rules/reuse-wrap-dont-leak.md) — Keep SDK types at the edge; your domain talks to a port you own

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|---|---|
| Keeping the chosen SDK replaceable | rs-ports-adapters |
| Crate features and workspace dependencies | rs-ecosystem |
| Retry, backoff, circuit breaker (don't hand-roll) | rs-domain-error |
| Gateway/wasm constraints | rs-domain-gateway |
| Finding hand-rolled standards in existing code | rs-review |
