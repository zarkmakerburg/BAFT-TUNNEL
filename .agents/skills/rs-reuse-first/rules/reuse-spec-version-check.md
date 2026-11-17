# reuse-spec-version-check

> Record which revision of the spec the chosen SDK implements and re-check on every major bump
> **rs-reuse-first · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Standards version themselves (MCP `2025-11-25`, `2026-07-28`; OAuth 2.1 drafts; OTLP versions). An SDK major release usually tracks a spec revision and changes API. If the version is not written down, nobody knows whether a client failure is a spec mismatch.

## Do

- Put the spec revision and the SDK version range in the decision record and in the crate README.
- Expose both in `--version` or a startup log line (rs-observability) — what is running is a fact.
- Subscribe to the SDK's release notes; schedule the upgrade, read the migration guide, update the adapter — not the domain.
- Test negotiation: an old client and a new client against your server (rs-testing).

## Bad

```toml
rmcp = "*"        # whichever version resolves today; spec revision unknown
```

## Good

```toml
# MCP spec 2026-07-28 (compatible with 2025-11-25) — decisions/0004-mcp-sdk.md
rmcp = { version = "3.9", default-features = false, features = ["server", "macros", "transport-streamable-http-server"] }
```

## See Also

- [reuse-official-sdk-first](reuse-official-sdk-first.md)
