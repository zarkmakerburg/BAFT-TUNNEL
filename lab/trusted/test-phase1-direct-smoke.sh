#!/usr/bin/env bash
set -Eeuo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/bin"
cat > "$tmp/bin/ssh" <<'MOCK'
#!/usr/bin/env bash
set -Eeuo pipefail
while (($#)); do
  case "$1" in
    -i|-p) (($# >= 2)) || exit 91; shift 2 ;;
    -o) (($# >= 2)) || exit 91; printf '%s\n' "$2" >> "$SSH_POLICY_LOG"; shift 2 ;;
    -*) echo "unexpected SSH option: $1" >&2; exit 92 ;;
    *) break ;;
  esac
done
(($# == 2)) || { echo 'SSH destination/command missing' >&2; exit 93; }
[[ "$1" == root@192.0.2.10 || "$1" == root@198.51.100.20 ]]
printf '%s\t%s\n' "$1" "$2" >> "$SSH_CALL_LOG"
case "$2" in
  *probe.py*) echo 'E2E_PAYLOAD_OK bytes=2097289 sha256=mock' ;;
esac
MOCK
cat > "$tmp/bin/scp" <<'MOCK'
#!/usr/bin/env bash
set -Eeuo pipefail
while (($#)); do
  case "$1" in
    -i|-P) (($# >= 2)) || exit 94; shift 2 ;;
    -o) (($# >= 2)) || exit 94; printf '%s\n' "$2" >> "$SSH_POLICY_LOG"; shift 2 ;;
    *) break ;;
  esac
done
(($# >= 2)) || { echo 'SCP sources/destination missing' >&2; exit 95; }
printf '%s\n' "$*" >> "$SCP_CALL_LOG"
MOCK
chmod +x "$tmp/bin/ssh" "$tmp/bin/scp"
printf 'mock-key\n' > "$tmp/key"
printf 'mock-host-keys\n' > "$tmp/known_hosts"
printf 'candidate-binary-not-executed\n' > "$tmp/baft-tunnel"
: > "$tmp/ssh.log"; : > "$tmp/scp.log"; : > "$tmp/policy.log"
export PATH="$tmp/bin:$PATH"
export SSH_CALL_LOG="$tmp/ssh.log" SCP_CALL_LOG="$tmp/scp.log" SSH_POLICY_LOG="$tmp/policy.log"
export LAB_IRAN_HOST=192.0.2.10 LAB_ABROAD_HOST=198.51.100.20
export LAB_SSH_PRIVATE_KEY=mock-private-key LAB_SSH_KNOWN_HOSTS=mock-host-keys
export LAB_IRAN_SSH_USER=root LAB_ABROAD_SSH_USER=root
export LAB_IRAN_SSH_PORT=22 LAB_ABROAD_SSH_PORT=22
export CANDIDATE_SHA=0123456789abcdef0123456789abcdef01234567
export CANDIDATE_BINARY="$tmp/baft-tunnel" GITHUB_RUN_ID=123
bash "$root/lab/trusted/phase1-direct-smoke.sh" > "$tmp/output.log"
grep -F 'TRUSTED_PHASE1_LAB=PASS' "$tmp/output.log" >/dev/null
grep -F 'candidate_sha=0123456789abcdef0123456789abcdef01234567' "$tmp/output.log" >/dev/null
test "$(grep -c 'probe.py' "$SSH_CALL_LOG")" = 2
test "$(grep -c 'serve --listen' "$SSH_CALL_LOG")" = 1
test "$(grep -c 'forward --listen' "$SSH_CALL_LOG")" = 1
test "$(wc -l < "$SCP_CALL_LOG" | tr -d ' ')" = 2
grep -F 'runuser -u nobody' "$SSH_CALL_LOG" >/dev/null
grep -F 'echo $$ >' "$SSH_CALL_LOG" >/dev/null
grep -F 'exec timeout --signal=TERM --kill-after=5s 180s' "$SSH_CALL_LOG" >/dev/null
if grep -Eqi 'bash .*phase1|lab/real-hosts|scripts/' "$SSH_CALL_LOG"; then
  echo 'Trusted harness attempted to execute candidate-controlled scripts' >&2; exit 1
fi
if grep -Eqi 'LAB_SSH_PRIVATE_KEY|mock-private-key' "$SSH_CALL_LOG" "$SCP_CALL_LOG"; then
  echo 'SSH private key crossed the LAB host boundary' >&2; exit 1
fi
grep -Fx 'StrictHostKeyChecking=yes' "$SSH_POLICY_LOG" >/dev/null
grep -Fx 'BatchMode=yes' "$SSH_POLICY_LOG" >/dev/null
echo 'Trusted Phase 1 LAB orchestration tests: PASS (mock SSH/SCP only; candidate binary not executed locally)'
