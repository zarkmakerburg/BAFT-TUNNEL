# reuse-official-sdk-first

> The SDK published by the owner of the spec or product beats every alternative
> **rs-reuse-first · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

The owner of a standard tracks its changes first, runs the conformance tests and answers issues. A community crate may be nicer to use, but it lags the spec and can be abandoned. When the official SDK exists, deviating needs a written reason.

## How to Recognise "Official"

| Signal | Example |
|---|---|
| crate's `repository` lives under the spec/vendor organisation | `rmcp` → `modelcontextprotocol/rust-sdk`; `clickhouse` → `ClickHouse/clickhouse-rs`; `async-nats` → `nats-io/nats.rs` |
| the spec site links to it | modelcontextprotocol.io → SDKs list |
| the vendor docs "Rust" page points at it | `aws-sdk-*` → AWS docs |

`sdk-scout.sh` marks these `OFFICIAL?` — confirm by reading the repo owner, a description saying "official" proves nothing on its own.

## Bad

```toml
# chose the crate whose name looked right, never checked who owns the spec
mcp-something = "0.1"     # last release 14 months ago, targets a 2024 spec revision
```

## Good

```toml
rmcp = { version = "3", default-features = false, features = ["server", "macros", "transport-io"] }
```
with a decision record noting the spec revision the SDK implements and why the features were chosen.

## When the Official SDK Falls Short

Contribute upstream or extend it in your adapter (rs-ports-adapters). Replacing it wholesale needs the decision record in [reuse-handroll-last-resort](reuse-handroll-last-resort.md).

## See Also

- [reuse-search-before-write](reuse-search-before-write.md)
- [reuse-spec-version-check](reuse-spec-version-check.md)
