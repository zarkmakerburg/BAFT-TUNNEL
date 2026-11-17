# ADR-0043: Rakhsh code copy policy (owner decision B-1)

Status: accepted (2026-11-17). Supersedes the "no copying" part of ADR-0038 clause 1 and 6; the rest of ADR-0038 stays. Relates to: ADR-0019, `RAKHSH_INTEGRATION_ANALYSIS.md`.

## Decision
The owner answered blocker B-1 on 2026-11-17: copy code if it is needed. Policy:
1. **Default stays independent rewrite** where it is cheap. Copying is allowed only where it clearly saves real work.
2. **Copied Rakhsh code never enters the proprietary core.** Any copied file lives in a separate AGPL-3.0-only unit (own crate or binary, own `LICENSE`, original copyright and notices kept, a `NOTICE` naming source repo, commit and path), following the ADR-0019 pattern: separate process, no shared linking, payload never crosses an IPC boundary to it.
3. If a copy could only work in-process with the core (it would force the core to become AGPL), that part is **not copied**; the feature is rewritten independently and the owner is asked only if a rewrite is not feasible.
4. The core's licence is unchanged (`LicenseRef-Proprietary`). Relicensing the core stays a separate owner decision.
5. `shabdiz` (Rakhsh's protocol crate) has an unknown licence and stays excluded until its licence is verified.

## Evaluation (2026-11-17, from file layout and docs, no source bodies opened)
| Candidate | In-process with data path? | Useful standalone? | Decision |
|---|---|---|---|
| Resume engine (`resume.rs`) | yes (stream framing and buffers) | no, bound to Rakhsh lanes and frames | **Not copied.** Copy would relicense the core. Independent rewrite on protocol v3 (ADR-0040) |
| Link/carrier manager (`links.rs`, `carrier.rs`) | yes (dial/accept loop) | no, bound to Rakhsh carriers and `shabdiz` | **Not copied.** `link_policy.rs` (ADR-0039) is the independent version |
| Health/status (`status.rs`) | yes | no | **Not copied.** `transport_health.rs` (ADR-0041) |
| Doctor (`doctor.rs`) | probes Rakhsh carriers | no, probes its own carriers | **Not copied.** Doctor is a wrapper over BAFT preflight (ADR-0042) |
| Test scripts (`scripts/*.py`) and unit tests | no (test tooling) | no, they drive the `rakhsh` binary | **Not copied.** Not applicable to BAFT |
| Raw/faketcp/L3, `encryption = "none"`, SSH fleet, bot | n/a | n/a | **Rejected** by security rules, unchanged |

Result: no file is both useful to BAFT and isolable, so nothing is copied today. The isolated-AGPL route stays available if a later candidate qualifies (for example a standalone tool run as a separate process); it needs a new entry in this table and an owner-visible PR.

## Consequences
The licence boundary and clean-room discipline for the in-process work are unchanged. Any future copy is a separate PR with `NOTICE`, AGPL `LICENSE` in its own directory, and a release note stating the source offer.
