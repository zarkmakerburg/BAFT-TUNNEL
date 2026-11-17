# NNNN — <topic>: <crate or "hand-rolled">

- **Date:** YYYY-MM-DD · **Status:** proposed | accepted | superseded by NNNN · **Deciders:** <who>
- **Skill:** rs-reuse-first

## Need

<Standard / product, roles needed (client | server | both), spec version, runtime constraints (tokio, wasm, no_std), non-goals.>

## Searched

| Where | Query | Result |
|---|---|---|
| registry/standards.md | `<row>` | <candidate> |
| `sdk-scout.sh "<term>" --github` | run on YYYY-MM-DD | <top hits with version, downloads, last release> |
| spec organisation GitHub | `<org>/rust-sdk`? | <found / none> |
| in-tree | `cargo tree -i <crate>` | <already present?> |

## Candidates

| | Official? | Spec version | Roles / transports | Last release | MSRV | Licence | Verdict |
|---|---|---|---|---|---|---|---|
| <crate A> | yes/no | | | | | | **chosen** |
| <crate B> | | | | | | | runner-up |
| hand-rolled | n/a | | | n/a | | n/a | rejected unless "Reason" below |

## Decision

<Chosen crate + version range + enabled features. One paragraph on why.>

## Gaps and how they are handled

<What the SDK does not cover → upstream issue/PR link | extension in our adapter | hand-rolled subset with spec-derived conformance tests.>

## Isolation

<Port trait that hides it (rs-ports-adapters), adapter module path, how it is swapped, contract tests that every implementation passes.>

## Reason to hand-roll (only if chosen)

<The hard constraint, the measurement, the exit plan "switch to <SDK> when …", and the conformance suite.>

## Consequences

<Dependencies added, MSRV impact, `cargo deny` result, what to re-check at each SDK major release.>
