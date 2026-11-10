#!/usr/bin/env bash
# Verify and install the latest signed BAFT-TUNNEL release on Ubuntu x86_64.
# One line: curl -fsSL https://raw.githubusercontent.com/zarkmakerburg/BAFT-TUNNEL/main/scripts/quick-install.sh | sudo bash -s -- --register t1
set -Eeuo pipefail
umask 077

REPO="zarkmakerburg/BAFT-TUNNEL"
API="https://api.github.com/repos/$REPO"
COSIGN_VERSION="v2.5.3"
COSIGN_SHA256="7a6b86904016cc271e854aee54ca5b8ec5bd7fe28369e8775025c7ece029e2a4"
COSIGN_IDENTITY='^https://github.com/zarkmakerburg/BAFT-TUNNEL/.github/workflows/release.yml@refs/tags/v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'
INSTANCE=""
AGENT_LISTEN=""
TAG=""
STABLE=0
VERIFY_ONLY=0

usage() {
  cat <<'USAGE'
BAFT-TUNNEL quick installer (Ubuntu Linux x86_64)

Usage:
  quick-install.sh --register NAME [--tag vX.Y.Z[-rc.N]] [--stable]
                   [--agent-listen 127.0.0.1:PORT]

Options:
  --register NAME       Required instance name; installs the local panel agent.
  --tag TAG             Install this exact published release (instead of latest).
  --stable              Select the latest stable release; default includes Preview.
  --agent-listen ADDR   Agent listen address (loopback only; default 127.0.0.1:18080).
  --verify-only         Verify and extract-check the signed package; do not install.
  -h, --help            Show this help.

The installer verifies the release's keyless Sigstore signature and SHA-256
before extracting or executing any package content. The tunnel is not started.
USAGE
}

die() { echo "baft-quick-install: $*" >&2; exit 2; }

github_api_get() {
  local url="$1"
  local -a args=(--fail --silent --show-error --location --retry 3
    -H 'Accept: application/vnd.github+json'
    -H 'User-Agent: baft-tunnel-installer')
  # CI supplies the workflow token to avoid anonymous API rate limits. The
  # public one-line installer remains usable without a token.
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    args+=(-H "Authorization: Bearer $GITHUB_TOKEN")
  fi
  curl "${args[@]}" "$url"
}

while (($#)); do
  case "$1" in
    --register) (($# >= 2)) || die "--register needs a name"; INSTANCE="$2"; shift 2 ;;
    --tag) (($# >= 2)) || die "--tag needs a value"; TAG="$2"; shift 2 ;;
    --stable) STABLE=1; shift ;;
    --agent-listen) (($# >= 2)) || die "--agent-listen needs an address"; AGENT_LISTEN="$2"; shift 2 ;;
    --verify-only) VERIFY_ONLY=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; die "unknown argument: $1" ;;
  esac
done

if (( VERIFY_ONLY == 0 )); then
  [[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "--register NAME is required (1-63 letters, digits, _ or -; starts with a letter or digit)"
elif [[ -n "$INSTANCE" ]]; then
  [[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,62}$ ]] || die "invalid --register instance name"
fi
if [[ -n "$TAG" ]]; then
  [[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "invalid release tag"
  (( STABLE == 0 )) || die "--tag and --stable cannot be combined"
fi
if [[ -n "$AGENT_LISTEN" ]]; then
  [[ "$AGENT_LISTEN" =~ ^(127\.0\.0\.1|\[::1\]):([0-9]{1,5})$ ]] &&
    (( BASH_REMATCH[2] >= 1 && BASH_REMATCH[2] <= 65535 )) ||
    die "--agent-listen must be 127.0.0.1:PORT or [::1]:PORT"
fi

[[ "$(uname -s)" == "Linux" ]] || die "only Linux hosts are supported"
case "$(uname -m)" in x86_64|amd64) ;; *) die "this release currently supports Linux x86_64 only" ;; esac
for tool in curl python3 tar sha256sum dpkg-deb awk grep sed mktemp chmod rm; do
  command -v "$tool" >/dev/null 2>&1 || die "missing required command: $tool"
done
if (( VERIFY_ONLY == 0 )); then
  [[ "$(id -u)" == "0" ]] || die "run with sudo/root"
  for tool in install systemctl; do
    command -v "$tool" >/dev/null 2>&1 || die "missing required command: $tool"
  done
fi

TMP_BASE="${TMPDIR:-/tmp}"
TMP="$(mktemp -d "$TMP_BASE/baft-quick-install.XXXXXX")"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT HUP INT TERM

echo "BAFT-TUNNEL: resolving the requested signed release..."
if [[ -n "$TAG" ]]; then
  RELEASE_JSON="$(github_api_get "$API/releases/tags/$TAG")" ||
    die "release $TAG was not found or GitHub API is unavailable"
else
  RELEASES_JSON="$(github_api_get "$API/releases?per_page=100")" ||
    die "cannot retrieve the GitHub releases list"
  RELEASE_JSON="$(python3 -c '
import json, re, sys
stable = sys.argv[1] == "1"
items = json.load(sys.stdin)
for item in items:
    tag = item.get("tag_name", "")
    if not item.get("draft") and (not stable or not item.get("prerelease")) and re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?", tag):
        print(json.dumps(item))
        break
' "$STABLE" <<<"$RELEASES_JSON")"
  [[ -n "$RELEASE_JSON" ]] || die "no published release found for the requested channel"
fi

RELEASE_META="$(python3 -c '
import json, sys
r = json.load(sys.stdin)
print(r.get("tag_name", ""))
print("true" if r.get("draft") else "false")
print("true" if r.get("prerelease") else "false")
' <<<"$RELEASE_JSON")"
TAG="$(printf '%s\n' "$RELEASE_META" | sed -n '1p')"
DRAFT="$(printf '%s\n' "$RELEASE_META" | sed -n '2p')"
PRERELEASE="$(printf '%s\n' "$RELEASE_META" | sed -n '3p')"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "GitHub returned an invalid release tag"
[[ "$DRAFT" == "false" ]] || die "refusing to install a draft release"
if (( STABLE == 1 )) && [[ "$PRERELEASE" != "false" ]]; then
  die "the selected release is a prerelease, not a stable release"
fi

NAME="baft-tunnel-$TAG-linux-x86_64"
BASE="https://github.com/$REPO/releases/download/$TAG"
ARCHIVE="$NAME.tar.gz"
echo "Release: $TAG"
[[ "$PRERELEASE" != "true" ]] || echo "Channel: Preview / prerelease"

echo "Downloading and verifying signed release artifacts..."
# Bootstrap a fixed, digest-pinned Cosign verifier; never trust a verifier
# downloaded from the same unsigned release being verified.
COSIGN_DEB="cosign_2.5.3_amd64.deb"
curl -fsSL --retry 3 "https://github.com/sigstore/cosign/releases/download/$COSIGN_VERSION/$COSIGN_DEB" -o "$TMP/$COSIGN_DEB"
printf '%s  %s\n' "$COSIGN_SHA256" "$TMP/$COSIGN_DEB" | sha256sum --check --status ||
  die "the pinned Cosign package failed its SHA-256 check"
mkdir "$TMP/cosign-root"
dpkg-deb -x "$TMP/$COSIGN_DEB" "$TMP/cosign-root"
# The Debian package ships /usr/bin/cosign as an absolute symlink to this file;
# invoking the symlink from the temporary extraction root would escape that root.
COSIGN_BIN="$TMP/cosign-root/usr/bin/cosign-linux-amd64"
[[ -x "$COSIGN_BIN" ]] || die "the verified Cosign package contains no executable at the pinned package path"

curl -fsSL --retry 3 "$BASE/$ARCHIVE" -o "$TMP/$ARCHIVE"
curl -fsSL --retry 3 "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS"
curl -fsSL --retry 3 "$BASE/SHA256SUMS.cosign.bundle" -o "$TMP/SHA256SUMS.cosign.bundle"
awk -v archive="$ARCHIVE" '$2 == archive && length($1) == 64 && $1 !~ /[^0-9a-f]/ { found++ } END { exit found != 1 }' "$TMP/SHA256SUMS" ||
  die "the signed checksum manifest does not contain exactly the requested release archive"

"$COSIGN_BIN" verify-blob "$TMP/SHA256SUMS" --bundle "$TMP/SHA256SUMS.cosign.bundle" \
  --certificate-identity-regexp "$COSIGN_IDENTITY" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
(
  cd "$TMP"
  sha256sum --ignore-missing --check SHA256SUMS
)

# Validate every tar member before extraction: only regular files/directories
# beneath the exact release root are accepted; links and special files are refused.
python3 - "$TMP/$ARCHIVE" "$NAME" <<'PYTAR'
import pathlib, sys, tarfile
archive, expected = sys.argv[1:]
with tarfile.open(archive, "r:gz") as tf:
    members = tf.getmembers()
    if not members:
        raise SystemExit("empty release archive")
    for m in members:
        p = pathlib.PurePosixPath(m.name)
        if p.is_absolute() or not p.parts or p.parts[0] != expected or ".." in p.parts:
            raise SystemExit(f"unsafe archive path: {m.name!r}")
        if not (m.isdir() or m.isfile()):
            raise SystemExit(f"unsupported archive entry type: {m.name!r}")
PYTAR

tar -xzf "$TMP/$ARCHIVE" -C "$TMP" --no-same-owner --no-same-permissions
PKG="$TMP/$NAME"
[[ -x "$PKG/baft-tunnel" && -x "$PKG/scripts/install.sh" ]] || die "verified package is missing required installer files"
"$PKG/baft-tunnel" --version >/dev/null 2>&1 || die "release binary failed its version check"
if (( VERIFY_ONLY == 1 )); then
  echo "Verified signed BAFT-TUNNEL release $TAG and package layout; no installation performed."
  exit 0
fi

if [[ -n "$AGENT_LISTEN" ]]; then
  bash "$PKG/scripts/install.sh" --binary "$PKG/baft-tunnel" --register "$INSTANCE" --agent-listen "$AGENT_LISTEN"
else
  bash "$PKG/scripts/install.sh" --binary "$PKG/baft-tunnel" --register "$INSTANCE"
fi

echo
echo "BAFT-TUNNEL $TAG installed and verified for instance $INSTANCE."
echo "Panel: use an SSH tunnel to 127.0.0.1:18080; read its token with: sudo cat /var/lib/baft-tunnel-agent/token"
echo "The tunnel itself was not started. Pair the two nodes from the panel or: sudo baft-tunnel menu"
