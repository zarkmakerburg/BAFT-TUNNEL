# ADR 0001: Independent project boundary

- Status: Accepted
- Date: 2026-11-03

## Decision
BAFT-TUNNEL is a clean, independent project. It does not fork BAFT and does not import BAFT code, Git history, credentials, release keys, production configuration, CI secrets, or deployment assets.

## Consequences
- New package/module namespace, signing root, database schema, migration history, CI, and release workflows.
- BAFT experience may inform requirements and acceptance tests only.
