#!/usr/bin/env bash
# curl | sudo bash safety on Ubuntu 24.04 (needrestart is installed and active there).
# Pipes a brace-grouped script, like quick-install.sh, into bash while a real apt install (which runs
# needrestart/dpkg and may read stdin) happens in the middle, and requires that the lines after it still run.
# It also checks that quick-install.sh and panel-https.sh keep the same shape. Needs root + apt (CI runner).
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
[[ "$(id -u)" == 0 ]] || { echo "needs root" >&2; exit 1; }
grep -q 'VERSION_ID="24.04"' /etc/os-release || echo "note: not Ubuntu 24.04; result is not 24.04 evidence" >&2
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
cat >"$T/piped.sh" <<'SCRIPT'
{
set -Eeuo pipefail
echo START_MARKER
# Deliberately no </dev/null and no NEEDRESTART_SUSPEND: the worst case the installer must survive.
DEBIAN_FRONTEND=noninteractive apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends certbot nginx-common
echo LINE_AFTER_APT_1
echo LINE_AFTER_APT_2
exit 0
}
SCRIPT
OUT="$(cat "$T/piped.sh" | bash 2>&1)" || { echo "$OUT" | tail -n 30; echo "FAIL: piped script failed" >&2; exit 1; }
for m in START_MARKER LINE_AFTER_APT_1 LINE_AFTER_APT_2; do
  grep -q "^$m\$" <<<"$OUT" || { echo "$OUT" | tail -n 30; echo "FAIL: $m lost" >&2; exit 1; }
done
# Shape guards for the real scripts.
test "$(grep -v '^#' "$ROOT/scripts/quick-install.sh" | sed -n '1p')" = '{'
test "$(tail -n1 "$ROOT/scripts/quick-install.sh")" = '}'
grep -Fq 'NEEDRESTART_SUSPEND=1 apt-get install' "$ROOT/scripts/panel-https.sh"
echo PIPE_INSTALL_2404=PASS
