# port-volatility-inventory

> List the changeable products at design time and decide the port for each
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Decoupling is cheap at design time and expensive after the first vendor type reaches the domain. A five-minute inventory during design tells you where ports are needed.

## Do

For a new service or feature, fill this in the plan (or the decision record) before writing code:

| Dependency | Replaceable by | Port name | Memory fake? | Contract suite? |
|---|---|---|---|---|
| analytics events | ClickHouse / DuckDB / files | `AuditStore` | yes | yes |
| session cache | Redis / Valkey / in-process | `SessionCache` | yes | yes |
| MCP transport | `rmcp` / future SDK | `ToolHost` | yes | handshake test |
| identity | OIDC provider A / B | `Authenticator` | yes | yes |

Then use `rs-reuse-first` to choose each adapter's SDK, and `rs-structure` to place `adapters/<product>/`.

## Ask of every row

*Who could force this to change — a licence, a price, a vendor, a scale limit, a better product?* If nobody plausibly can and it needs no fake, it is not a port ([port-yagni](port-yagni.md)).

## See Also

- [port-design-by-need](port-design-by-need.md)
