#!/usr/bin/env bash
# sdk-scout — search the market BEFORE writing a protocol / standard / vendor client yourself.
#
#   sdk-scout.sh <term> [<term>...] [--limit N] [--github] [--manifest path/to/Cargo.toml]
#
# What it does
#   1. crates.io search per term, sorted by downloads  (name, version, downloads, recent, last release, repo)
#   2. flags candidates:  OFFICIAL? (repo owner is the spec/vendor org, or the description says "official"),
#                         STALE (no release for > 12 months), IN-TREE (already a dependency of your manifest)
#   3. --github: also lists top GitHub repositories (needs `gh`), because the official SDK is sometimes not
#      on crates.io under the name you would guess
#   4. prints the evaluation checklist and where to record the decision
#
# It only reads public registry metadata. Nothing from your project is sent anywhere except the search terms.
set -euo pipefail

LIMIT=6; GITHUB=0; MANIFEST=""; TERMS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --limit) LIMIT="$2"; shift 2;;
    --github) GITHUB=1; shift;;
    --manifest) MANIFEST="$2"; shift 2;;
    -h|--help) sed -n '2,16p' "$0"; exit 0;;
    *) TERMS+=("$1"); shift;;
  esac
done
[[ ${#TERMS[@]} -gt 0 ]] || { echo "usage: sdk-scout.sh <term>... [--limit N] [--github] [--manifest Cargo.toml]" >&2; exit 2; }
command -v curl >/dev/null || { echo "curl is required" >&2; exit 2; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 2; }

if [[ -z "$MANIFEST" ]]; then
  d="$PWD"; while [[ "$d" != "/" ]]; do [[ -f "$d/Cargo.toml" ]] && { MANIFEST="$d/Cargo.toml"; break; }; d="$(dirname "$d")"; done
fi

# Orgs whose repositories ARE the standard's own SDK. Extend as the ecosystem grows.
OFFICIAL_ORGS="modelcontextprotocol grpc tokio-rs hyperium rustls open-telemetry opentelemetry awslabs aws Azure googleapis google-cloud-rust ClickHouse mongodb redis-rs elastic opensearch-project nats-io apache bytecodealliance kube-rs cedar-policy microsoft proxy-wasm rust-lang rust-random serde-rs RustCrypto hickory-dns testcontainers a2aproject kafka"

for term in "${TERMS[@]}"; do
  echo
  echo "=== crates.io: \"$term\" (top $LIMIT by downloads) ==="
  curl -fsS -A "rs-skills sdk-scout (generic client)" -G "https://crates.io/api/v1/crates" \
       --data-urlencode "q=$term" --data-urlencode "sort=downloads" --data-urlencode "per_page=$LIMIT" 2>/dev/null \
  | OFFICIAL_ORGS="$OFFICIAL_ORGS" MANIFEST="$MANIFEST" python3 -I -c '
import json, os, re, sys, datetime
orgs = set(os.environ["OFFICIAL_ORGS"].lower().split())
deps = set()
m = os.environ.get("MANIFEST")
if m and os.path.isfile(m):
    root = os.path.dirname(m)
    texts = [open(m, errors="ignore").read()]
    for dp, dn, fn in os.walk(root):
        dn[:] = [d for d in dn if d not in ("target", ".git", "node_modules")]
        if "Cargo.toml" in fn and os.path.join(dp, "Cargo.toml") != m:
            texts.append(open(os.path.join(dp, "Cargo.toml"), errors="ignore").read())
    for t in texts:
        for mm in re.finditer(r"^\s*([A-Za-z0-9_-]+)\s*(?:=|\.workspace)", t, re.M):
            deps.add(mm.group(1).replace("_", "-").lower())
try:
    crates = json.load(sys.stdin)["crates"]
except Exception:
    print("  (registry unreachable or rate-limited - retry, or search lib.rs / docs.rs / GitHub by hand)"); sys.exit(0)
if not crates:
    print("  no crate matched - try the protocol/vendor name, the RFC number, or --github"); sys.exit(0)
now = datetime.datetime.now(datetime.timezone.utc)
for c in crates:
    repo = (c.get("repository") or "")
    owner = ""
    mm = re.search(r"github\.com/([^/]+)/", repo)
    if mm: owner = mm.group(1).lower()
    desc = (c.get("description") or "")
    flags = []
    if owner in orgs or re.search(r"\bofficial\b", desc, re.I): flags.append("OFFICIAL?")
    try:
        upd = datetime.datetime.fromisoformat(c["updated_at"].replace("Z", "+00:00"))
        if (now - upd).days > 365: flags.append("STALE")
        updated = c["updated_at"][:10]
    except Exception: updated = "?"
    if c["name"].replace("_", "-").lower() in deps: flags.append("IN-TREE")
    ver = c.get("max_stable_version") or c.get("max_version")
    print("  %-34s %-12s dl=%-12s recent=%-11s rel=%s %s" % (c["name"], ver, c["downloads"], c.get("recent_downloads"), updated, " ".join(flags)))
    print("      %s" % (desc[:110] or "(no description)"))
    if repo: print("      %s" % repo)
'
  sleep 1   # crates.io crawler policy: at most 1 request/second
  if [[ $GITHUB -eq 1 ]]; then
    echo
    echo "=== GitHub: \"$term\" rust (top $LIMIT by stars) ==="
    if command -v gh >/dev/null; then
      gh api -X GET search/repositories -f q="$term language:rust in:name,description" -f sort=stars -f per_page="$LIMIT" \
        --jq '.items[] | "  \(.full_name)  ★\(.stargazers_count)  pushed \(.pushed_at[:10])\(if .archived then "  ARCHIVED" else "" end)\n      \(.description // "(no description)")"' 2>/dev/null \
        || echo "  (gh search failed - check \`gh auth status\`)"
    else
      echo "  (gh not installed - search https://github.com/search?q=$term+rust&s=stars by hand)"
    fi
  fi
done

cat <<'TXT'

--- Before you pick one (rs-reuse-first) -------------------------------------------------------------
 1. Official first: the SDK published by the organisation that owns the spec / the vendor beats everything.
 2. Does it implement the spec version you must support?  (read its README + CHANGELOG, not the crate name)
 3. Does it cover the roles you need (client AND server, every transport, auth)?  Gaps => contribute / wrap, not rewrite.
 4. Health: release < 12 months, open-issue responsiveness, MSRV vs yours, licence (cargo deny), unsafe/geiger.
 5. Hand-rolling is the LAST resort and needs a written reason. Record the decision:
      skills/rs-reuse-first/templates/decision-record.md  ->  <docs>/decisions/NNNN-<topic>.md
 6. Whatever you pick, keep it behind a port you own (rs-ports-adapters) so it stays replaceable.
------------------------------------------------------------------------------------------------------
TXT
