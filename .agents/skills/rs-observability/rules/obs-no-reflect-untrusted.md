# obs-no-reflect-untrusted

> Never put the content you just blocked or masked into the reply, a header, or an INFO log
> **rs-observability · rule** — source: rs-skills, learned on the MCP Flex gateway policies (2026-11)

## Why It Matters

A filter that blocks a prompt injection and then writes `"blocked: Content: \"ignore previous
instructions…\""` into the error message has delivered the injection anyway. The same applies
to masked values in debug lines, e-mail addresses in INFO logs, and claim values in traces.

## Bad

```rust
format!("Tool '{tool}' blocked — injection detected. Content: \"{preview}\"")   // reply
logger::info!("HOLD tool={tool} user_email={email}")                         // PII at INFO
logger::debug!("claims={claims_json}")                                       // every claim value
```

## Good

```rust
format!("Tool '{tool}' blocked — prompt injection detected (category: {category}).")
logger::info!("HOLD tool={tool} agent_id={agent}")                // ids, not personal data
logger::debug!("claim_names={}", claim_names(&claims_json))       // names, never values
```

Keep evidence (previews, matched values) only in protected audit channels that need it
(e.g. a stream property the audit policy reads), never in what the caller receives.

## See Also

- [obs-no-sensitive-data](obs-no-sensitive-data.md)
- rs-error-handling `err-fail-closed`
