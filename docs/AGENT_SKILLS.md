# Agent Skills installed for BAFT-TUNNEL

**Installed:** 2026-11-17 in the isolated project clone; project-local path: .agents/skills/.
**Lock:** [skills-lock.json](../skills-lock.json) pins every skill to upstream commit
999cb52ec82d4ddfc719398fa991a837f471f931 from
[bassem-elsodany/rs-skills](https://github.com/bassem-elsodany/rs-skills/tree/999cb52ec82d4ddfc719398fa991a837f471f931).
The install used copy mode, so the selected skill files are self-contained in this repository.

## Selected skills

| Skill | Intended use in BAFT-TUNNEL |
|---|---|
| **rs-security-review** | Threat-model-first review of Rust services, input boundaries, DoS, secrets and supply-chain risks |
| **rs-bugfix** | Reproduce a defect, identify the invariant, fix it and search for sibling defects |
| **rs-testing** | Unit, integration, property, async and regression-test design |
| **rs-fuzzing** | Fuzz parsers, decoders, configuration and protocol boundaries |
| **rs-review** | Diff-risk triage and structured code review |
| **rs-observability** | Truthful health, freshness, structured logs and telemetry |
| **rs-domain-distributed** | Distributed-system failure modes and network fault modelling |
| **rs-async-patterns** | Tokio async design, cancellation, backpressure and bounded work |
| **rs-concurrency** | Locks, atomics, task coordination and concurrency safety |
| **rs-ports-adapters** | Keep transports and vendor integrations behind replaceable boundaries |
| **rs-reuse-first** | Check maintained, official crates and standards before implementing protocols by hand |
| **rs-style** | Rust idioms, formatting and lint discipline |

## Integration and limitations

- These are project-local Agent Skills for Codex and compatible local agents. The installer verified all 12 under .agents/skills/.
- They are not automatically loaded by the ChatGPT Android app. The current mobile chat does not gain filesystem skills merely because they are committed here.
- The upstream bundle is primarily authored for Claude Code. This installation contains the selected Markdown skill folders only; it does not include upstream hooks, agents, slash commands or bundle-level scripts.
- The self-contained helper rs-review/scripts/diff-review.py was used for the initial PR #323 diff triage. Do not run rs-review/scripts/review-report.sh as a complete machine pass: it references omitted bundle-level scripts and companion skills.
- rs-security-review links to optional companion skills not installed in this subset (rs-unsafe, rs-domain-auth, rs-domain-policy, rs-domain-gateway, rs-release). Use the included checklists and rules, but do not claim those omitted passes were completed.
- Generic skill advice never overrides this repository's AGENTS.md, PROJECT_PROFILE.md, docs/HQ_USAGE.md, accepted ADRs, threat model, or owner permissions. In particular: no CAP_NET_ADMIN; no firewall/route/sysctl changes; no real-host LAB without task-specific owner authorization; no merge, release or deploy without its explicit gate.

## Security and licence review

- The 12 selected directories contain 195 files including their nested rules, guides, templates and helper scripts.
- Static pattern review found no remote curl-pipe-shell or wget-pipe-shell execution, sudo/doas, network-admin commands, credential-file paths, dynamic eval/exec, or base64-decoding commands.
- One rm -rf occurrence is confined to a mktemp scratch directory in the optional rs-review report script; that script is not enabled as a complete workflow here. No upstream helper script was executed during installation. The read-only diff-review.py was the only installed helper executed.
- Upstream LICENSE and NOTICE.md are preserved beside the skills. The selected content is MIT-licensed; the upstream rs-patterns directory, which carries MPL-2.0 material, was not installed.
- Other candidates were reviewed but not copied: openai/skills is deprecated; trailofbits/skills has a repository-level CC BY-SA licence and was kept outside this product repository; addyosmani/agent-skills is more web/Node oriented; mattpocock/skills substantially overlaps the existing Superpowers debugging, testing and review workflows.

Updates must be reviewed, pinned to a specific upstream commit, and installed through a PR. No automatic skill updates are enabled.
