# Phase 0 Exit Evidence — Owner Review Candidate

**Status:** Candidate package for owner review. This document does not declare Phase 0 accepted and does not authorize a phase transition.  
**Project:** BAFT-TUNNEL (independent private project)  
**Evidence branch:** `phase0/six-host-lab-foundation`  
**Evidence commit:** `8e1e9e18d41df74d467b0345e0bb1683f7d61876`  
**Pull request:** [PR #1 — Phase 0: add six-host BAFT-TUNNEL test lab](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/1)

## 1. Phase 0 scope

Phase 0 is limited to architecture, threat model, repository governance/foundation, and network-lab foundation. It does not include a working tunnel engine, production installer, control plane, or web panel.

## 2. Architecture and decisions

- [Architecture baseline](ARCHITECTURE.md) — components, trust boundaries, transport baseline, identity, desired state, reliability and phase boundaries.
- [Threat model](THREAT_MODEL.md) — assets, trust boundaries, adversaries, mitigations and explicit limitations.
- [ADR 0001](decisions/0001-independent-project.md) — independent project boundary; no BAFT source, history, credentials or operational artifacts.
- [ADR 0002](decisions/0002-transport-baseline.md) — pooled TLS 1.3 mutual-authenticated TCP transport baseline; implementation remains subject to Phase 1 review.
- [ADR 0003](decisions/0003-rust-core.md) — Rust-only core implementation; no Phase 1 authorization implied.
- [Security Guide](SECURITY_GUIDE.md) and [Engineering Guardrails](ENGINEERING_GUARDRAILS.md) — binding safety and engineering requirements.
- [Product UX and Installer Acceptance](PRODUCT_UX_ACCEPTANCE.md) — binding D-008 simplicity, full dashboard information architecture and usability/accessibility acceptance.

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
- The real-host workflow is manual and read-only, guarded by the exact `READ_ONLY_PREFLIGHT` confirmation string.
- Mock tests verify all 14 SSH invocations, strict host-key options, IPv4/IPv6 input, duplicate rejection, invalid values, and that remote OS metadata is parsed as data rather than sourced as shell code.

The owner has not configured the six-host GitHub Environment variables/secrets. **No real host was contacted or modified.** The actual six-host preflight remains NOT RUN.

## 4. CI evidence tied to the exact commit

Exact commit: `8e1e9e18d41df74d467b0345e0bb1683f7d61876`

- [Workflow run 41781588190](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41781588190) — success.
- [Workflow run 41781593696](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/41781593696) — success.

Successful checks include:

1. POSIX and Bash syntax validation.
2. Immutable GitHub Actions reference enforcement and negative/positive policy tests.
3. LAB control-script tests.
4. NetEm cleanup failure-path tests (mock Docker only).
5. Six-host preflight tests (mock SSH only).
6. Docker Compose validation.
7. Isolated Docker/netem integration, including cleanup.

Earlier failed intermediate runs are preserved in GitHub Actions history. They exposed test-harness and policy-parser defects that were corrected and covered by regression tests; they are not deleted or relabeled.

## 5. Repository and governance evidence

- [HQ Control Board — Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2) is the canonical operations/decisions/evidence record.
- [PR #1](https://github.com/zarkmakerburg/BAFT-TUNNEL/pull/1) remains Draft/open and mergeable clean.
- The PR base/main commit remains `1a53751ce186ea8263a11ac264130d0544d0627b`; main has not been changed by this work.
- Workflows use a full immutable checkout action SHA, disable persisted checkout credentials, and request only read-only repository permissions.
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
| Threat model and security constraints | Documented; Phase 0 baseline, review still welcome |
| Independent repo and governance | PR #1, Issue #2, private repo; main unchanged |
| Engineering guardrails | Documented and linked from README |
| Product simplicity and dashboard IA | D-008 + PRODUCT_UX_ACCEPTANCE.md |
| Reproducible isolated network simulation | Exact-SHA CI runs successful |
| Six-host LAB foundation | Scripts, workflow, docs and mock tests present |
| Actual six-host test | NOT RUN — owner has not configured LAB |
| Tunnel implementation | NOT IN PHASE 0 / NOT STARTED |

## 8. Owner gate

Owner review is requested for the Phase 0 evidence package only.

If the owner accepts Phase 0, a separate explicit instruction is required to authorize Phase 1. Merge of PR #1, Phase 1 implementation, release, deployment and production changes remain separate gates and are not implied by acceptance of this document.

**Current command state:** remain in Phase 0; do not merge or begin Phase 1 without explicit owner approval.
