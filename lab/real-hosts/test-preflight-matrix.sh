#!/usr/bin/env bash
set -Eeuo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cat > "$tmp/ssh" <<'MOCK'
#!/bin/sh
printf '%s\n' "$*" >> "$SSH_CALL_LOG"
printf '%s\n' 'MOCK_REMOTE_OK'
MOCK
chmod +x "$tmp/ssh"
printf 'mock-key\n' > "$tmp/key"
printf 'mock.example ssh-ed25519 AAAATEST\n' > "$tmp/known_hosts"
export PATH="$tmp:$PATH" SSH_CALL_LOG="$tmp/ssh.log"
export SSH_KEY_FILE="$tmp/key" SSH_KNOWN_HOSTS_FILE="$tmp/known_hosts"
export IR_1_HOST=192.0.2.11 IR_2_HOST=192.0.2.12
export EX_1_HOST=198.51.100.21 EX_2_HOST=198.51.100.22 EX_3_HOST=198.51.100.23 EX_4_HOST=198.51.100.24
: > "$SSH_CALL_LOG"
output=$(bash "$root/lab/real-hosts/preflight-matrix.sh")
grep -F '6 hosts inspected; 8 Iran-to-abroad paths checked' <<<"$output" >/dev/null
test "$(wc -l < "$SSH_CALL_LOG" | tr -d ' ')" = 14
grep -F -- '-o StrictHostKeyChecking=yes' "$SSH_CALL_LOG" >/dev/null
grep -F -- "-o UserKnownHostsFile=$SSH_KNOWN_HOSTS_FILE" "$SSH_CALL_LOG" >/dev/null
: > "$SSH_CALL_LOG"
if EX_4_HOST="$EX_3_HOST" bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'duplicate host unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
: > "$SSH_CALL_LOG"
if LAB_TEST_PORTS='443;id' bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'unsafe port list unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
echo 'Six-host matrix preflight tests: PASS (mock SSH only; no network connection made)'
