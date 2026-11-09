# Merge policy and `main` protection

Status: protection ACTIVE on `main` (applied 2026-11-09 by the owner's login; verified via API:
`protected=true`, checks `rust`, `ubuntu-2204-baseline`, `lab-config`, 0 approvals, no force-push,
no deletion, admins not enforced). Re-apply or verify with the sections below.

## Rules for `main`
- Changes land only through a pull request; no direct pushes, no force pushes, no deletion.
- Required status checks (names from `phase0.yml` and `phase1-rust.yml`, which run on every PR to `main`):
  `rust`, `ubuntu-2204-baseline`, `lab-config`.
- Path-filtered workflows (`fuzz-smoke`, `baft-engine-conformance`) are **not** required, because a
  required check that never runs on a PR would block it. They must be green when they do run.
- Approving reviews: **0 required**. The owner is the only human; threads merge their own PRs on green CI.
- Admins may bypass (`enforce_admins=false`) so the owner is never locked out. Use only for emergencies and note it in the PR.
- "Require branches to be up to date" is off to avoid churn between parallel threads.
- Repository stays public.

## Merge flow
1. Branch from `main`, open a (draft) PR with evidence in the description.
2. Wait for the three required checks to pass on the PR head SHA.
3. Squash-merge; never merge on red; never skip or quarantine a test to get green.
4. Release tags and LAB runs use the exact merged `main` SHA and record it as evidence.

## Re-applying
Script (repo-admin `gh` login): `scripts/apply-branch-protection.sh`.
UI: Settings → Branches → Add branch ruleset/protection rule for `main` → require a pull request
(0 approvals), require status checks `rust`, `ubuntu-2204-baseline`, `lab-config`, block force pushes
and deletions, leave "Do not allow bypassing" unchecked.

## Verify
`gh api repos/zarkmakerburg/BAFT-TUNNEL/branches/main --jq '{protected,checks:.protection.required_status_checks.contexts}'`
expected: `{"protected":true,"checks":["rust","ubuntu-2204-baseline","lab-config"]}`.
