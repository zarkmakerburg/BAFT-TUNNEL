#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
SCRIPT="$ROOT/scripts/quick-install.sh"
[[ -x "$SCRIPT" ]]
HELP="$("$SCRIPT" --help)"
grep -Fq -- '--register NAME' <<<"$HELP"
grep -Fq -- '--tag TAG' <<<"$HELP"
grep -Fq -- '--stable' <<<"$HELP"
grep -Fq -- '--verify-only' <<<"$HELP"
grep -Fq '7a6b86904016cc271e854aee54ca5b8ec5bd7fe28369e8775025c7ece029e2a4' "$SCRIPT"
grep -Fq 'verify-blob "$TMP/SHA256SUMS"' "$SCRIPT"
grep -Fq 'sha256sum --ignore-missing --check SHA256SUMS' "$SCRIPT"
grep -Fq 'awk -v archive="$ARCHIVE"' "$SCRIPT"
grep -Fq 'COSIGN_BIN="$TMP/cosign-root/usr/bin/cosign-linux-amd64"' "$SCRIPT"
grep -Fq 'Authorization: Bearer $GITHUB_TOKEN' "$SCRIPT"
grep -Fq 'github_api_get "$API/releases/tags/$TAG"' "$SCRIPT"
grep -Fq 'tarfile' "$SCRIPT"
grep -Fq 'bash "$PKG/scripts/install.sh" --binary "$PKG/baft-tunnel" --register "$INSTANCE"' "$SCRIPT"
if "$SCRIPT" --tag '../main' --register test >/dev/null 2>&1; then
  echo 'unsafe release tag was accepted' >&2; exit 1
fi
if "$SCRIPT" --register 'bad.name' >/dev/null 2>&1; then
  echo 'unsafe instance name was accepted' >&2; exit 1
fi
if "$SCRIPT" --tag v0.1.0-rc.6 >/dev/null 2>&1; then
  echo 'missing required instance registration was accepted' >&2; exit 1
fi
if "$SCRIPT" --tag v0.1.0-rc.6 --stable --register t1 >/dev/null 2>&1; then
  echo 'conflicting stable and pinned release selection was accepted' >&2; exit 1
fi
if "$SCRIPT" --register t1 --agent-listen 0.0.0.0:18080 >/dev/null 2>&1; then
  echo 'non-loopback agent listen address was accepted' >&2; exit 1
fi
echo QUICK_INSTALL_TEST=PASS
