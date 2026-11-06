#!/usr/bin/env bash
set -Eeuo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cat > "$tmp/ssh" <<'MOCK'
#!/bin/sh
printf '%s\n' "$*" >> "$SSH_CALL_LOG"
cat >> "$SSH_STDIN_LOG"
printf '%s\n' 'MOCK_REMOTE_OK'
MOCK
chmod +x "$tmp/ssh"
printf 'mock-key\n' > "$tmp/key"
printf 'mock.example ssh-ed25519 AAAATEST\n' > "$tmp/known_hosts"
export PATH="$tmp:$PATH" SSH_CALL_LOG="$tmp/ssh.log" SSH_STDIN_LOG="$tmp/ssh-stdin.log"
export SSH_KEY_FILE="$tmp/key" SSH_KNOWN_HOSTS_FILE="$tmp/known_hosts"
export LAB_IRAN_HOST=192.0.2.11 LAB_ABROAD_HOST=2001:db8::24
export LAB_IRAN_SSH_USER=root LAB_ABROAD_SSH_USER=labuser
export LAB_IRAN_SSH_PORT=2222 LAB_ABROAD_SSH_PORT=22
: > "$SSH_CALL_LOG"
: > "$SSH_STDIN_LOG"
output=$(bash "$root/lab/real-hosts/preflight-matrix.sh")
grep -F 'TWO-HOST PREFLIGHT COMPLETE' <<<"$output" >/dev/null
test "$(wc -l < "$SSH_CALL_LOG" | tr -d ' ')" = 3
test "$(grep -c 'os_name=$(awk' "$SSH_STDIN_LOG")" = 2
if grep -F '. /etc/os-release' "$SSH_STDIN_LOG" >/dev/null; then echo 'remote OS metadata must not be sourced as shell code' >&2; exit 1; fi
test "$(grep -c -- '-o StrictHostKeyChecking=yes' "$SSH_CALL_LOG")" = 3
test "$(grep -c -- "-o UserKnownHostsFile=$SSH_KNOWN_HOSTS_FILE" "$SSH_CALL_LOG")" = 3
grep -F -- '-p 2222' "$SSH_CALL_LOG" >/dev/null
grep -F -- '-p 22' "$SSH_CALL_LOG" >/dev/null
grep -F 'root@192.0.2.11' "$SSH_CALL_LOG" >/dev/null
grep -F 'labuser@2001:db8::24' "$SSH_CALL_LOG" >/dev/null
grep -F "python3 - '2001:db8::24' '443,8443,2053,18300' '5'" "$SSH_CALL_LOG" >/dev/null

: > "$SSH_CALL_LOG"
if LAB_ABROAD_HOST="$LAB_IRAN_HOST" bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'duplicate host unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
if LAB_ABROAD_HOST='2001:::24' bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'invalid IPv6 unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
if LAB_TEST_PORTS='443;id' bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'unsafe port list unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
if LAB_IRAN_SSH_PORT=65536 bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'invalid Iran SSH port unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
echo 'Two-host preflight tests: PASS (mock SSH only; IPv4/IPv6 input; per-host SSH settings; no network connection made)'
