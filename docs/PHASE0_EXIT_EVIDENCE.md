# Phase 0 Exit Evidence — Owner Review Candidate

**Status:** Historical Phase 0 snapshot from 2026-11-04. Its original state statements are retained for traceability and are superseded by the current addendum at the end of this document.
**Project:** BAFT-TUNNEL (independent private project)  
**Historical evidence branch:** `phase0/six-host-lab-foundation`
**Phase 0 baseline commit:** `8e1e9e18d41df74d467b0345e0bb1683f7d61876`  
**Latest PR head at this revision:** `2ce68dbc9aeda3798cd40848ee80ca17fc439be2` (documentation-only evidence reconciliation)  
**Latest code/behavior commit reviewed:** `95b7da3020618bda07a1c9d96b0d46f204834a6f`  
**Pull request:** [PR #1 — Phase 0: add six-host BAFT-TUNNEL test lab](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/1)

## 1. Phase 0 scope

Phase 0 is limited to architecture, threat model, repository governance/foundation, and network-lab foundation. It does not include a working tunnel engine, production installer, control plane, or web panel.

## 2. Architecture and decisions

- [Architecture baseline](ARCHITECTURE.md) — components, trust boundaries, transport baseline, identity, desired state, reliability and phase boundaries.
- [Threat model](THREAT_MODEL.md) — assets, trust boundaries, adversaries, mitigations and explicit limitations.
- [ADR 0001](decisions/0001-independent-project.md) — independent project boundary; no BAFT source, history, credentials or operational artifacts.
- [ADR 0002](decisions/0002-transport-baseline.md) — pooled TLS 1.3 mutual-authenticated TCP transport baseline; implementation remains subject to Phase 1 review.
- [ADR 0003](decisions/0003-rust-core.md) — Rust-only core implementation; no Phase 1 authorization implied.
- [Security Guide](SECURITY_GUIDE.md) and [Engineering Guardrails](ENGINEERING_GUARDRAILS.md) — binding safety and engineering requirements, including risk-proportionate testing.
- [Product UX and Installer Acceptance](PRODUCT_UX_ACCEPTANCE.md) — binding D-008 simplicity, dashboard information architecture and usability/accessibility acceptance, plus D-009 GitHub Release publishing from this ChatGPT environment.

## 3. Network-lab evidence

### Isolated Docker/netem simulation

The Phase 0 CI workflow builds two isolated Linux containers and verifies:

- baseline container connectivity (3/3 ping replies);
- application of Linux `tc netem` at 40 ms delay / 2% loss;
- qdisc state observation;
- TCP iperf3 sender/receiver completion;
- qdisc removal and verification;
- container and isolated network cleanup.

This is **container simulation only**. It is not a real server-to-server test, not a real Iran-to-abroad path, and not evidence of tunnel throughput or production performance.

### Six-host preflight foundation

- Two Iran host slots and four abroad host slots are defined.
- The preflight models all eight directed Iran→abroad TCP paths.
- IPv4, IPv6 literal and DNS host inputs are supported subject to documented constraints.
- SSH uses strict pinned host keys and a dedicated low-privilege account.
- The real-host workflow is manual and read-only, guarded by the exact `READ_ONLY_PREFLIGHT` confirmation string. A separate validation job fails incorrect confirmation before the protected environment or host credentials are reached.
- Existing mock checks verify all 14 SSH invocations, strict host-key options, IPv4/IPv6 input, duplicate rejection, invalid values, and that remote OS metadata is parsed as data rather than sourced as shell code.

The owner has not configured the six-host GitHub Environment variables/secrets. **No real host was contacted or modified.** The actual six-host preflight remains NOT RUN. Per the approved Phase 0 scope, missing host configuration does not authorize inventing host data or accessing real hosts.

## 4. CI evidence tied to exact commits

- [Run 41791351045](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41791351045) — **completed / success** on code/behavior commit `95b7da3020618bda07a1c9d96b0d46f204834a6f`.
- [Run 41823513508](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41823513508) — **completed / success** on documentation reconciliation commit `2ce68dbc9aeda3798cd40848ee80ca17fc439be2`.
- [Workflow-fix push run 41790089193](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41790089193) and [PR run 41790096469](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41790096469) — success on exact workflow-fix SHA `2b9c8c73093262b318b01975a659411f5f0c8481`.
- [Earlier baseline run 41781588190](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41781588190) and [41781593696](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41781593696) — successful baseline checks.
- D-009 documentation commit `51a48d539c566002cd6da000de21ea9c4f067847` was verified by [run 41785951457](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41785951457) and [run 41785959557](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41785959557), both successful on that exact SHA.

The latest verified runs execute the existing repository checks:

1. POSIX and Bash syntax validation.
2. Immutable GitHub Actions reference enforcement and policy fixtures.
3. Existing LAB control-script checks.
4. Existing NetEm cleanup failure-path checks (mock Docker only).
5. Existing six-host preflight checks (mock SSH only).
6. Docker Compose validation.
7. Isolated Docker/netem integration and cleanup.

The test-proportionality policy was added in commit `95b7da3`. **That documentation-only change added zero tests**; CI ran the existing suite. Tests are selected by concrete risk and acceptance value, not added for test count or CI decoration.

Earlier failed intermediate runs remain in GitHub Actions history. They exposed test-harness and policy-parser defects that were corrected and covered by the then-existing checks; they are not deleted or relabeled.

## 5. Repository and governance evidence

- [HQ Control Board — Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) is the canonical operations/decisions/evidence record.
- [PR #1](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/1) remains Draft/open and mergeable; it is not merged.
- PR base/main remains `1a53751ce186ea8263a11ac264130d0544d0627b`; main has not been changed by this work.
- The PR branch is ahead of main with no base-branch divergence; see the live PR for the current commit count.
- Workflows use a full immutable checkout action SHA, disable persisted checkout credentials, and request only read-only repository permissions.
- The manual real-host preflight validates exact confirmation in a separate no-secrets job; invalid confirmation fails before the protected LAB environment or host credentials are reached.
- The isolated LAB image and direct network-test packages are pinned.
- No secrets, private keys, real host addresses or production configuration were added to the repository.

## 6. Explicitly unproven / out of scope

The following are **NOT VERIFIED** and must not be described as completed:

- a Rust tunnel engine or production-ready binary;
- real authenticated tunnel establishment or forwarded application traffic;
- actual six-host SSH preflight;
- real Iran-to-abroad connectivity, throughput, latency or packet-loss behavior;
- failover, recovery under real path loss, or censorship-resistance;
- installer, upgrade, rollback, backup/restore or uninstall behavior;
- panel/API/agent implementation;
- independent security certification or production readiness.

## 7. Phase 0 acceptance candidate

| Criterion | Evidence / status |
|---|---|
| Architecture and technology baseline | Documented; ADRs 0001–0003 |
| Threat model and security constraints | Documented as Phase 0 baseline; implementation review remains required |
| Independent repo and governance | PR #1, Issue #2, private repo; main unchanged |
| Engineering guardrails | Documented, linked from README; test scope is risk-proportionate |
| Product simplicity and dashboard IA | D-008 + PRODUCT_UX_ACCEPTANCE.md |
| GitHub Release publishing from this ChatGPT environment | D-009 recorded as binding requirement; implementation intentionally deferred to Phase 5 |
| Reproducible isolated network simulation | Exact-SHA runs 41791351045 and 41823513508 successful |
| Six-host LAB foundation | Scripts, workflow, docs and existing mock checks present |
| Actual six-host test | NOT RUN — owner has not configured LAB |
| Tunnel implementation | NOT IN PHASE 0 / NOT STARTED |

## 8. Owner gate

Owner review is requested for the Phase 0 evidence package only.

If the owner accepts Phase 0, a separate explicit instruction is required to authorize Phase 1. Merge of PR #1, Phase 1 implementation, release, deployment and production changes remain separate gates and are not implied by acceptance of this document.

**Current command state:** remain in Phase 0; do not merge or begin Phase 1 without explicit owner approval.


## 9. Current status addendum — 2026-11-06

This addendum supersedes the original snapshot above for current project status. The original sections remain as historical evidence and must not be read as today's operational state.

### Current phase and repository state

- Phase 0 architecture, repository, mock/simulation lab and two-host read-only preflight foundation have been delivered.
- The owner explicitly authorized Phase 1; Phase 1 implementation and validation are active.
- Main is currently at `d9bbc46`. PR #1 remains Draft/open and its Phase 0 branch is being reconciled with main before any merge. PR #3 is Draft/open and targets `phase0/six-host-lab-foundation`.
- The active LAB is one configured Iran host and one configured abroad host. The former six-host matrix is optional future work, not the active configuration.

### Verified real-host evidence

- Phase 0 read-only preflight: [run 42221903167](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42221903167) passed. Both hosts reported Ubuntu 22.04.x and expected baseline tools. TCP probes on 443, 8443, 2053 and 18300 reported host-reachable/no-listener. No persistent host changes were made.
- Phase 1 Direct/Reverse tunnel E2E: [run 42273060260](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42273060260) checked out runtime commit `ee1c224718dd201fd050f98eacb788ebd0387219` and passed Direct and Reverse forwarding, matching 2,097,289-byte payload hashes, listener restart/recovery, and rejection of revoked client and server certificates. Temporary processes/files were cleaned up. This is bounded LAB evidence, not production readiness.
- Latest Phase 1 head is `0d05e349277a9481b1433694f4cdd9d21142d696`. Its Phase 1 Rust CI passed in [run 42274280120](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42274280120); Phase 0 workflow CI passed in [runs 42274280441](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42274280441) and [42274272173](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/42274272173).

### Still NOT VERIFIED

- Encrypted systemd credential delivery using `LoadCredentialEncrypted=`: the two active LAB hosts run Ubuntu 22.04 / systemd 249 and cannot prove the systemd 250+ feature.
- Online enrollment/control-plane authorization, automated CRL distribution and rollover, expanded adversarial/fuzz testing, persistent Linux service lifecycle, and soak testing.
- Production readiness, long-duration reliability, censorship resistance, and any production deployment.

### Current phase gate

Phase 1 is active and owner-authorized, but PR #3 remains Draft. No merge, release, deployment, or persistent production-host mutation is implied by this addendum. Continue with the Phase 1 acceptance queue in the [HQ Control Board](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2).
