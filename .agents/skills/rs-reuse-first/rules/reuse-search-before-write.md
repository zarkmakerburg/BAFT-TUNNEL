# reuse-search-before-write

> Search crates.io, GitHub and the spec's own organisation before writing any protocol or vendor client
> **rs-reuse-first · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

An agent asked for "an MCP server" will happily draft the JSON-RPC envelope, capability negotiation and transports from memory. It works for `initialize` + one tool call and fails on everything else — versioning, cancellation, progress, pagination, auth, streamable HTTP. The user then has to throw it away and redo it on the official SDK. Knowledge of a protocol is not a substitute for the maintained implementation of it.

## Bad

```rust
// "MCP server" written from memory
#[derive(Deserialize)]
struct Request { jsonrpc: String, id: Value, method: String, params: Option<Value> }

async fn handle(line: &str) -> String {
    let req: Request = serde_json::from_str(line).unwrap();
    match req.method.as_str() {
        "initialize" => /* hand-written capabilities… */ todo!(),
        "tools/list" => todo!(),
        "tools/call" => todo!(),
        _ => todo!(), // every other method, notification, error code, version …
    }
}
```

## Good

```
$ skills/rs-reuse-first/scripts/sdk-scout.sh "model context protocol" --github
  rmcp 3.9.0  OFFICIAL?  github.com/modelcontextprotocol/rust-sdk
```

```rust
// shape of the official SDK's server setup (README of modelcontextprotocol/rust-sdk, rmcp 3.x)
use rmcp::ServiceExt;
use tokio::io::{stdin, stdout};

let service = MyHandler::new();              // implements rmcp's `ServerHandler`; tools via the #[tool] macros
let server  = service.serve((stdin(), stdout())).await?;   // transport + protocol lifecycle are the SDK's job
server.waiting().await?;
```
Use the SDK's current `examples/` for the exact handler and macro names of the version you pin — and keep `MyHandler` thin: it calls your port (rs-ports-adapters).

## Procedure

1. Name the standard, the roles, the spec version.
2. `registry/standards.md`, then `scripts/sdk-scout.sh "<term>" --github`.
3. Check the spec organisation's GitHub for `rust-sdk`.
4. Report what you found to the user **before** writing code ("Reuse report" in the skill).

## See Also

- [reuse-official-sdk-first](reuse-official-sdk-first.md)
- [reuse-evaluate-candidate](reuse-evaluate-candidate.md)
