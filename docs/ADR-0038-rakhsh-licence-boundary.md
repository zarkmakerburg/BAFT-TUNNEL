# ADR-0038: Rakhsh licence boundary and clean-room rules

Status: accepted (2026-11-17, owner's delegated authority; owner may overrule, see blocker B-1). Relates to: ADR-0019, `RAKHSH_INTEGRATION_ANALYSIS.md`.

## Context
Rakhsh (`CluvexStudio/rakhsh`) is AGPL-3.0-only. The BAFT-TUNNEL Rust core is proprietary, all rights reserved, and ADR-0019 says it never links AGPL code. Copying or adapting Rakhsh source would make the core a derivative work with network copyleft obligations.

## Decision
1. No Rakhsh source, tests, scripts, identifiers or comments enter this repository.
2. Features are implemented independently from BAFT ADRs. Generic techniques (exponential backoff with cap, hysteresis, TCP-style byte offsets with cumulative acknowledgement, EWMA smoothing) are public-domain engineering and may be used.
3. Authors of the implementation PRs work from the ADRs and the user-facing behaviour descriptions, not from Rakhsh source bodies. Source bodies of carrier, links, resume, doctor modules were not opened for this analysis.
4. Tests are written from the ADR acceptance criteria, not translated from Rakhsh tests.
5. Each PR states: no Rakhsh code copied. A reviewer who finds similarity beyond the generic techniques blocks the PR.
6. Relicensing, depending on `shabdiz`, or copying code requires a new ADR and the owner's explicit decision.

## Consequences
Slower than porting, but the licence position of the core is unchanged. This is an engineering control, not legal advice; counsel review is advised if the owner wants to copy code.
