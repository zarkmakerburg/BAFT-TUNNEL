#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cat > "$tmp/docker" <<'MOCK'
#!/bin/sh
printf '%s\n' "$*" >> "$DOCKER_CALL_LOG"
MOCK
chmod +x "$tmp/docker"
export DOCKER_CALL_LOG="$tmp/docker.log"
export PATH="$tmp:$PATH"
: > "$DOCKER_CALL_LOG"
"$root/lab/scripts/apply-netem.sh" mild endpoint-b >/dev/null
grep -F -- "compose -f $root/lab/scripts/../compose.yaml exec -T endpoint-b tc qdisc replace dev eth0 root netem delay 40ms loss 2% rate 100mbit" "$DOCKER_CALL_LOG" >/dev/null
if "$root/lab/scripts/apply-netem.sh" bogus >/dev/null 2>&1; then echo 'invalid profile unexpectedly succeeded' >&2; exit 1; fi
if "$root/lab/scripts/apply-netem.sh" mild endpoint-c >/dev/null 2>&1; then echo 'invalid endpoint unexpectedly succeeded' >&2; exit 1; fi
: > "$DOCKER_CALL_LOG"
"$root/lab/scripts/show-netem.sh" >/dev/null
test "$(wc -l < "$DOCKER_CALL_LOG" | tr -d ' ')" = 2
: > "$DOCKER_CALL_LOG"
"$root/lab/scripts/clear-netem.sh" >/dev/null
test "$(wc -l < "$DOCKER_CALL_LOG" | tr -d ' ')" = 2
echo 'LAB script tests: PASS'
