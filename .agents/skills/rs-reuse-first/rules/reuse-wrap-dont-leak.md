# reuse-wrap-dont-leak

> Keep SDK types at the edge; your domain talks to a port you own
> **rs-reuse-first · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Choosing the SDK is not the same as marrying it. SDKs release breaking majors (`rmcp` ships a 3.x migration guide), vendors disappear, licences change. If SDK types are in your domain, every such event is a rewrite.

## Bad

```rust
// domain/policy.rs
use rmcp::model::CallToolResult;           // vendor type in the domain
pub fn decide(r: &CallToolResult) -> bool { /* … */ }
```

## Good

```rust
// domain/policy.rs — no SDK imports
pub struct ToolOutcome { pub allowed: bool, pub reason: Option<String> }

// adapters/mcp/rmcp.rs — the only place that names rmcp types
impl From<&rmcp::model::CallToolResult> for ToolOutcome { /* … */ }
```

How to build the port, the adapter and the contract tests: rs-ports-adapters.

## See Also

- [reuse-official-sdk-first](reuse-official-sdk-first.md)
