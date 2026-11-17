# reuse-migrate-handrolled

> Replace an existing hand-rolled standard with the SDK in compiling, tested steps
> **rs-reuse-first · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

The drafted implementation is already in the repo and other code depends on it. A big-bang rewrite breaks clients; keeping it forever keeps the bugs. Migrate behind a port with characterisation tests, one operation at a time.

## Steps

1. **Pin behaviour.** Record the current wire behaviour (golden requests/responses, error shapes) as characterisation tests — [rs-structure `restructure-existing`](../../rs-structure/SKILL.md) uses the same discipline.
2. **Introduce the port.** Extract a trait (rs-ports-adapters) that the domain calls; the hand-rolled code becomes adapter `legacy`.
3. **Add the SDK adapter** next to it, behind a Cargo feature or runtime config.
4. **Run both** against the characterisation tests; list the differences. Differences that are bugs in the legacy code get fixed by the SDK; differences clients rely on are decided with the user (the contract wins — rs-api-design `api-config-is-contract`).
5. **Switch the default**, keep `legacy` for one release, then delete it and its tests in a separate commit (rs-git).
6. Record the decision and the removal in the CHANGELOG.

## Detecting Candidates

`skills/rs-review/scripts/risk-scan.py` lists "reinvention candidates" (JSON-RPC envelopes, hand-written SSE/frame parsing, token splitting, vendor REST calls) when the matching SDK is not a dependency.

## See Also

- [reuse-handroll-last-resort](reuse-handroll-last-resort.md)
