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
export IR_1_HOST=192.0.2.11 IR_2_HOST=192.0.2.12
export EX_1_HOST=198.51.100.21 EX_2_HOST=198.51.100.22 EX_3_HOST=198.51.100.23 EX_4_HOST=2001:db8::24
: > "$SSH_CALL_LOG"
: > "$SSH_STDIN_LOG"
output=$(bash "$root/lab/real-hosts/preflight-matrix.sh")
grep -F '6 hosts inspected; 8 Iran-to-abroad paths checked' <<<"$output" >/dev/null
test "$(wc -l < "$SSH_CALL_LOG" | tr -d ' ')" = 14
test "$(grep -c 'os_name=$(awk' "$SSH_STDIN_LOG")" = 6
if grep -F '. /etc/os-release' "$SSH_STDIN_LOG" >/dev/null; then echo 'remote OS metadata must not be sourced as shell code' >&2; exit 1; fi
test "$(grep -c -- '-o StrictHostKeyChecking=yes' "$SSH_CALL_LOG")" = 14
test "$(grep -c -- "-o UserKnownHostsFile=$SSH_KNOWN_HOSTS_FILE" "$SSH_CALL_LOG")" = 14
for target in 198.51.100.21 198.51.100.22 198.51.100.23 '2001:db8::24'; do
  test "$(grep -F -c "python3 - '$target' '443,8443,2053,18300' '5'" "$SSH_CALL_LOG")" = 2
done
test "$(grep -c 'bafttunnel-lab@192.0.2.11' "$SSH_CALL_LOG")" = 5
test "$(grep -c 'bafttunnel-lab@192.0.2.12' "$SSH_CALL_LOG")" = 5
: > "$SSH_CALL_LOG"
if EX_4_HOST="$EX_3_HOST" bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'duplicate host unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
if EX_4_HOST='2001:::24' bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'invalid IPv6 unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
if LAB_TEST_PORTS='443;id' bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'unsafe port list unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
if SSH_PORT=65536 bash "$root/lab/real-hosts/preflight-matrix.sh" >/dev/null 2>&1; then echo 'invalid SSH port unexpectedly accepted' >&2; exit 1; fi
test ! -s "$SSH_CALL_LOG"
echo 'Six-host matrix preflight tests: PASS (mock SSH only; IPv4/IPv6 input; no network connection made)'
