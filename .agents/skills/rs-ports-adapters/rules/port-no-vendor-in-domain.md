# port-no-vendor-in-domain

> Business code never names a vendor crate; dependencies point inward
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

If `use clickhouse::Client` appears in a service, replacing ClickHouse means reading every service. If it appears only in `adapters/clickhouse/`, replacing it means one directory. The dependency direction is the whole pattern: adapters depend on the domain, never the reverse.

## Bad

```rust
// src/audit/service.rs
use clickhouse::Client;                       // vendor in business code
pub struct AuditService { ch: Client }
```

## Good

```rust
// src/audit/service.rs
pub struct AuditService<S: AuditStore> { store: S }   // depends on the port only
```
```
core/            Cargo.toml has NO sqlx / redis / clickhouse / rmcp
adapter-clickhouse/   depends on core + clickhouse
app/             depends on core + adapters, wires them in main
```

## Enforce It

- Single crate: `python3 -I scripts/port-check.py . --strict` in CI (rs-release gate).
- Workspace: a core crate without vendor dependencies makes the compiler enforce it.
- Review: a vendor `use` outside `adapters/`/wiring is a finding (rs-review).

## See Also

- [port-design-by-need](port-design-by-need.md)
- [port-select-at-root](port-select-at-root.md)
