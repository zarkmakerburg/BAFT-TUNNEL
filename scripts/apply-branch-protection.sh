#!/usr/bin/env bash
# Applies the main-branch protection described in docs/MERGE_POLICY.md.
# Needs repo-admin gh auth (run by the owner). Idempotent.
set -euo pipefail
REPO="${1:-zarkmakerburg/BAFT-TUNNEL}"
gh api -X PUT "repos/${REPO}/branches/main/protection" --input - <<'JSON'
{"required_status_checks":{"strict":false,"contexts":["rust","ubuntu-2204-baseline","lab-config"]},
 "enforce_admins":false,
 "required_pull_request_reviews":{"required_approving_review_count":0,"dismiss_stale_reviews":false,"require_code_owner_reviews":false},
 "restrictions":null,
 "allow_force_pushes":false,"allow_deletions":false}
JSON
gh api "repos/${REPO}/branches/main" --jq '{protected,checks:.protection.required_status_checks.contexts}'
