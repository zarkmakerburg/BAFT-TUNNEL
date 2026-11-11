# BAFT-TUNNEL HQ Project Adapter

The reusable HQ methodology lives in its independent repository: <https://github.com/zarkmakerburg/project-hq>.

This directory contains only BAFT-TUNNEL-specific bindings. Do not duplicate or fork the HQ core here.

## Read in this order

1. [Project HQ agent entry point](https://github.com/zarkmakerburg/project-hq/blob/main/AGENTS.md) — reusable operating contract, HQ ON/OFF/STATUS semantics and safety boundaries.
2. [BAFT-TUNNEL project profile](PROJECT_PROFILE.md) — repository identity, canonical board, project-specific continuation shorthand and environment boundaries.
3. Accepted BAFT-TUNNEL ADRs and roadmap — product architecture, priorities and acceptance criteria.

## Separation rule

Project HQ is a reusable operating method, not BAFT-TUNNEL product functionality. This adapter must contain only project-specific repository identifiers, boards, environment registration and authorization boundaries. Never transfer these bindings to another project. The HQ core does not grant repository, machine, LAB, production, merge, release or deployment permissions.
