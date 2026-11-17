# reuse-handroll-last-resort

> Hand-roll only with a written reason, spec-derived tests and an exit plan
> **rs-reuse-first · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

There are real reasons to hand-roll: a wasm sandbox that cannot link the SDK, `no_std`, a measured hot path, a protocol nobody has implemented. In those cases the cost is permanent — you now own the spec's moving parts. The exception must be small, tested against the spec and reversible.

## Acceptable Reasons (each needs evidence)

| Reason | Evidence |
|---|---|
| No SDK exists | `sdk-scout` output, spec org check, date |
| Constraint excludes every candidate | build error for the target, e.g. `wasm32-wasip1` link failure; licence verdict |
| Measured performance gap | benchmark of SDK vs need (rs-performance) |

Not acceptable: "faster to write", "I know the protocol", "the SDK has too many features" (disable them), "avoid a dependency" (you replaced a dependency with unmaintained code).

## Do

1. **Subset only.** Implement what you use; reject the rest explicitly (`method not found`, version mismatch), never silently.
2. **Types from the spec.** Derive messages and error codes from the spec/schema; keep the schema file in the repo; generate when possible (`typify`, `prost-build`).
3. **Conformance fixtures.** Golden request/response pairs from the spec or the reference implementation; run against both the SDK's client (if it compiles on the host) and your code.
4. **Fail closed.** Unknown version/capability → refuse (rs-error-handling `err-fail-closed`).
5. **Exit plan.** Decision record names the trigger ("switch to the SDK when it supports wasm") and the port that makes the swap local (rs-ports-adapters).
6. **Design Contract.** The module's contract says "hand-rolled subset of <spec> <version>; see decisions/NNNN".

## See Also

- [reuse-search-before-write](reuse-search-before-write.md)
- [reuse-migrate-handrolled](reuse-migrate-handrolled.md)
