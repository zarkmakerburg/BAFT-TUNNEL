#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cat > "$tmp/docker" <<'MOCK'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$DOCKER_CALL_LOG"
case " $* " in
  *" ps --status running --services "*)
    printf '%s\n' endpoint-a endpoint-b
    ;;
  *" exec -T endpoint-a tc qdisc show dev eth0 "*)
    if [ -f "$DOCKER_STATE.endpoint-a" ]; then
      printf '%s\n' 'qdisc netem 10: root delay 40ms loss 2%'
    else
      printf '%s\n' 'qdisc fq_codel 0: root'
    fi
    ;;
  *" exec -T endpoint-b tc qdisc show dev eth0 "*)
    if [ -f "$DOCKER_STATE.endpoint-b" ]; then
      printf '%s\n' 'qdisc netem 10: root delay 40ms loss 2%'
    else
      printf '%s\n' 'qdisc fq_codel 0: root'
    fi
    ;;
  *" exec -T endpoint-a tc qdisc del dev eth0 root "*)
    rm -f "$DOCKER_STATE.endpoint-a"
    ;;
  *" exec -T endpoint-b tc qdisc del dev eth0 root "*)
    rm -f "$DOCKER_STATE.endpoint-b"
    ;;
  *" exec -T "*" tc -s qdisc show dev eth0 "*)
    printf '%s\n' 'qdisc fq_codel 0: root'
    ;;
esac
MOCK
chmod +x "$tmp/docker"
export DOCKER_CALL_LOG="$tmp/docker.log"
export DOCKER_STATE="$tmp/netem-state"
export PATH="$tmp:$PATH"
: > "$DOCKER_CALL_LOG"
sh "$root/lab/scripts/apply-netem.sh" mild endpoint-b >/dev/null
grep -F -- "compose -f $root/lab/scripts/../compose.yaml exec -T endpoint-b tc qdisc replace dev eth0 root netem delay 40ms loss 2% rate 100mbit" "$DOCKER_CALL_LOG" >/dev/null
if sh "$root/lab/scripts/apply-netem.sh" bogus >/dev/null 2>&1; then echo 'invalid profile unexpectedly succeeded' >&2; exit 1; fi
if sh "$root/lab/scripts/apply-netem.sh" mild endpoint-c >/dev/null 2>&1; then echo 'invalid endpoint unexpectedly succeeded' >&2; exit 1; fi
: > "$DOCKER_CALL_LOG"
sh "$root/lab/scripts/show-netem.sh" >/dev/null
test "$(wc -l < "$DOCKER_CALL_LOG" | tr -d ' ')" = 2
: > "$DOCKER_CALL_LOG"
: > "$DOCKER_STATE.endpoint-a"
: > "$DOCKER_STATE.endpoint-b"
sh "$root/lab/scripts/clear-netem.sh" >/dev/null
test "$(grep -c 'ps --status running --services' "$DOCKER_CALL_LOG")" = 1
test "$(grep -c 'tc qdisc show dev eth0' "$DOCKER_CALL_LOG")" = 4
test "$(grep -c 'tc qdisc del dev eth0 root' "$DOCKER_CALL_LOG")" = 2
test ! -e "$DOCKER_STATE.endpoint-a"
test ! -e "$DOCKER_STATE.endpoint-b"
echo 'LAB script tests: PASS'
