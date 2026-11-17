# reuse-evaluate-candidate

> Score candidates on spec fit, roles, health and licence — not on first impressions
> **rs-reuse-first · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

"Most downloads" and "nicest README" both mislead: downloads include transitive pulls, and a README can describe a spec revision two versions old. A 10-minute scorecard avoids a month of rework.

## Scorecard

| Check | How | Fail means |
|---|---|---|
| **Owner** | repo org vs spec/vendor org | not official → need a reason to pick it over the official one |
| **Spec version** | README / CHANGELOG / docs.rs for the revision named; the spec's own "SDKs" page | cannot interoperate with current clients |
| **Roles and transports** | client? server? stdio / HTTP / WebSocket? auth? | you will hand-roll the missing half |
| **Release cadence** | last release < 12 months (`sdk-scout` shows `STALE`); open issues answered | abandoned; you inherit the maintenance |
| **Adoption** | recent downloads, reverse dependencies (`crates.io` "dependents"), used by the official examples | niche; bus factor 1 |
| **Fit** | runtime (tokio?), `default-features`, MSRV ≤ yours, `no_std`/wasm support if needed | integration friction |
| **Licence** | `cargo deny check licenses` | cannot ship |
| **Safety** | `cargo geiger` / `unsafe` count, `cargo audit` | review burden |
| **API stability** | semver history, breaking-change frequency, migration guide | churn cost — plan the adapter around it |

## Good

Two columns in the decision record: chosen and runner-up. Hand-rolling appears as a third column and must lose on a *measured or sourced* fact.

## See Also

- [reuse-official-sdk-first](reuse-official-sdk-first.md)
- [reuse-handroll-last-resort](reuse-handroll-last-resort.md)
