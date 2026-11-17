# Agent entry point — BAFT-TUNNEL

This repository is the BAFT-TUNNEL **product** (Rust). It is not the HQ playbook repository.

- HQ playbook (generic method): https://github.com/zarkmakerburg/project-hq — read its `AGENTS.md` when HQ is ON.
- This project's HQ profile: [PROJECT_PROFILE.md](PROJECT_PROFILE.md)
- How to use HQ here (commands, repo roles, evidence, limits): [docs/HQ_USAGE.md](docs/HQ_USAGE.md)
- Canonical operations / decisions / evidence board: [Issue #2](https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2)
- Roadmap: [docs/PRODUCT_ROADMAP.md](docs/PRODUCT_ROADMAP.md)

Do not scan the repository; start from the files above.


## Installed Agent Skills

Selected Rust Agent Skills are installed project-locally in .agents/skills/; the exact upstream
commit and file hashes are pinned in skills-lock.json. See docs/AGENT_SKILLS.md for the selected
skills, scope, limitations and licence notices. Use them only when relevant.
This repository's profile, HQ usage guide, accepted ADRs and owner permissions take precedence.
Do not invoke optional bundle-level automation whose dependencies are not installed.
