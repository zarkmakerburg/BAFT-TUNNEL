#!/usr/bin/env bash
set -Eeuo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cat > "$tmp/docker" <<'MOCK'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$DOCKER_CALL_LOG"
case " $* " in
  *" ps --status running --services "*)
    if [ "${PS_FAIL:-0}" = 1 ]; then exit 41; fi
    if [ "${PROJECT_RUNNING:-1}" = 1 ]; then
      printf '%s\n' endpoint-a endpoint-b
    fi
    ;;
  *" exec -T "*" tc qdisc show dev eth0 "*)
    if [ -f "$DOCKER_STATE" ]; then
      printf '%s\n' 'qdisc netem 10: root delay 40ms loss 2%'
    else
      printf '%s\n' 'qdisc fq_codel 0: root'
    fi
    ;;
  *" exec -T "*" tc qdisc del dev eth0 root "*)
    if [ "${DELETE_FAIL:-0}" = 1 ]; then exit 42; fi
    rm -f "$DOCKER_STATE"
    ;;
esac
MOCK
chmod +x "$tmp/docker"
export PATH="$tmp:$PATH"
export DOCKER_CALL_LOG="$tmp/docker.log"
export DOCKER_STATE="$tmp/netem-state"
: > "$DOCKER_CALL_LOG"
: > "$DOCKER_STATE"

# Normal cleanup removes the qdisc and verifies the resulting state.
sh "$root/lab/scripts/clear-netem.sh"
test ! -e "$DOCKER_STATE"
grep -q 'tc qdisc del dev eth0 root' "$DOCKER_CALL_LOG"

# A failed qdisc delete must fail the script, not be swallowed.
: > "$DOCKER_STATE"
: > "$DOCKER_CALL_LOG"
if DELETE_FAIL=1 sh "$root/lab/scripts/clear-netem.sh" >/dev/null 2>&1; then
  echo 'failed qdisc delete unexpectedly reported success' >&2
  exit 1
fi

# A Docker inspection error must also fail closed.
if PS_FAIL=1 sh "$root/lab/scripts/clear-netem.sh" >/dev/null 2>&1; then
  echo 'failed Docker inspection unexpectedly reported success' >&2
  exit 1
fi

# A stopped Compose project has no running qdisc and needs no exec call.
: > "$DOCKER_CALL_LOG"
PROJECT_RUNNING=0 sh "$root/lab/scripts/clear-netem.sh"
if grep -q ' exec ' "$DOCKER_CALL_LOG"; then
  echo 'stopped project unexpectedly received exec calls' >&2
  exit 1
fi

echo 'NetEm cleanup failure-path tests: PASS (mock Docker only; delete/inspection failures fail closed)'
