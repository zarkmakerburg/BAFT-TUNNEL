#!/usr/bin/env bash
# Record the newest signed stable BAFT-TUNNEL release for the panel's update notice (ADR-0034).
# Operator-run (or from the menu / a timer the operator adds). The agent itself never goes online:
# it only reads the small file this script writes. Nothing is installed or changed on the host.
#
#   update-check.sh [--state-dir DIR] [--api-json FILE]
#
# --state-dir  where update-check.json is written (default /var/lib/baft-tunnel-agent)
# --api-json   read the GitHub "latest release" JSON from FILE instead of the network (tests)
set -Eeuo pipefail
umask 022

REPO="zarkmakerburg/BAFT-TUNNEL"
STATE_DIR="/var/lib/baft-tunnel-agent"
API_JSON=""
die() { echo "update-check: $*" >&2; exit 1; }

while (($#)); do
  case "$1" in
    --state-dir) (($# >= 2)) || die "--state-dir needs a path"; STATE_DIR="$2"; shift 2 ;;
    --api-json) (($# >= 2)) || die "--api-json needs a file"; API_JSON="$2"; shift 2 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

if [[ -n "$API_JSON" ]]; then
  body="$(cat "$API_JSON")"
else
  body="$(curl -fsSL --max-time 20 --retry 2 -H 'Accept: application/vnd.github+json' \
    "https://api.github.com/repos/$REPO/releases/latest")" || die "could not reach the GitHub release API; nothing written"
fi

# /releases/latest never returns a prerelease or draft; refuse one anyway.
grep -Eq '"prerelease"[[:space:]]*:[[:space:]]*false' <<<"$body" || die "latest release is not a stable release; nothing written"
tag="$(grep -Eo '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' <<<"$body" | head -n1 | sed -E 's/.*"([^"]*)"$/\1/')"
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "unexpected tag '${tag:0:40}'; nothing written"

[[ -d "$STATE_DIR" ]] || die "state directory $STATE_DIR does not exist"
tmp="$(mktemp "$STATE_DIR/.update-check.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
printf '{"latest_tag":"%s","checked_unix":%s,"source":"github-releases-latest"}\n' "$tag" "$(date +%s)" >"$tmp"
chmod 0644 "$tmp"
mv -f "$tmp" "$STATE_DIR/update-check.json"
trap - EXIT
echo "latest stable release: $tag"
