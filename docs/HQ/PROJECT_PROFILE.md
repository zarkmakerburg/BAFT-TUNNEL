# Project Profile — BAFT-TUNNEL

This file is the project-specific adapter for the reusable HQ operating methodology in this directory. It is not product architecture or runtime functionality. When reusing HQ in another project, create a new profile there; do not copy this project's identifiers, board, environments, approvals or operational facts.

## Identity

- **Project name:** BAFT-TUNNEL
- **Repository:** https://github.com/zarkmakerburg/BAFT-TUNNEL
- **Default branch:** main
- **HQ policy location:** AGENTS.md and docs/HQ/
- **Canonical operational/evidence board:** https://github.com/zarkmakerburg/BAFT-TUNNEL/issues/2
- **Product roadmap:** docs/PRODUCT_ROADMAP.md
- **Project status snapshot:** docs/PROJECT_BOARD.md

## Project-specific boundaries

- This profile binds the generic HQ process to BAFT-TUNNEL only. It does not grant repository, account, machine, LAB, production, merge, release or deployment permissions.
- Keep BAFT-TUNNEL independent from the separate BAFT project. Transfer only explicitly accepted lessons and requirements; do not import BAFT source, history, credentials, deployment state or secrets.
- Project-specific architecture, technical decisions and acceptance criteria belong in the project's ADRs and roadmap, not in the reusable HQ methodology.
- Project-specific continuation shorthand: in BAFT-TUNNEL conversations, a single or double period means continue from the latest verified project state and perform the next safe, authorized engineering action.

## Environments and host registry

- **Registered real-host LAB inventory:** NOT DECLARED IN THIS PROFILE.
- **Production inventory:** NOT DECLARED IN THIS PROFILE.
- **Access mechanism / pinned host identities:** NOT VERIFIED HERE.
- **Operational consequence:** No agent may infer or assume a host is LAB. Until an owner-reviewed project-specific registry and task procedure identify the exact target role, scope, authorization, expected effects, cleanup and rollback, real-host operations are BLOCKED. Unlisted or unidentified hosts are out of scope and must be treated as production.
- Never store host addresses, credentials, private keys or secret values in this file. Use protected project settings and refer only to non-secret role identifiers.

## Project-level delivery gates

Merge, release, signing, publishing and deployment follow the current repository rules and explicit project-owner authorization. This profile does not independently authorize any of them.
