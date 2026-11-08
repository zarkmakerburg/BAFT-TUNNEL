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
(($# == 2)) || { echo 'SSH destination/command missing or options appeared after destination' >&2; exit 93; }
destination="$1"
remote_command="$2"
[[ "$destination" == root@192.0.2.10 || "$destination" == root@198.51.100.20 ]]
printf '%s\t%s\n' "$destination" "$remote_command" >> "$SSH_CALL_LOG"
case "$remote_command" in
  *check-ports.py*) echo 'LAB test ports are free' ;;
  # Refusal scenarios: the 4096-byte probe and the busy-port second node must fail.
  *probe.py*4096*) exit 1 ;;
  *'timeout 15'*) exit 1 ;;
  *probe.py*) echo 'E2E_PAYLOAD_OK bytes=2097289 sha256=mock elapsed_s=0.01' ;;
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
cat > "$tmp/bin/sha256sum" <<'MOCK'
#!/bin/sh
shasum -a 256 "$@"
MOCK
chmod +x "$tmp/bin/"*

printf 'mock-key\n' > "$tmp/key"
printf 'mock-host-keys\n' > "$tmp/known_hosts"
printf 'mock-binary\n' > "$tmp/baft-tunnel"
chmod 700 "$tmp/baft-tunnel"
: > "$tmp/ssh.log"
: > "$tmp/scp.log"
: > "$tmp/ssh-policy.log"

export PATH="$tmp/bin:$PATH"
export SSH_KEY_FILE="$tmp/key"
export SSH_KNOWN_HOSTS_FILE="$tmp/known_hosts"
export SSH_CALL_LOG="$tmp/ssh.log"
export SCP_CALL_LOG="$tmp/scp.log"
export SSH_POLICY_LOG="$tmp/ssh-policy.log"
export LAB_IRAN_HOST=192.0.2.10
export LAB_ABROAD_HOST=198.51.100.20
export LAB_IRAN_SSH_USER=root
export LAB_ABROAD_SSH_USER=root
export LAB_IRAN_SSH_PORT=22
export LAB_ABROAD_SSH_PORT=22
export GITHUB_RUN_ID=mock-123
export BAFT_TUNNEL_BINARY="$tmp/baft-tunnel"

bash "$root/lab/real-hosts/phase1-two-host-e2e.sh" > "$tmp/output.log"
grep -F 'PHASE1_TWO_HOST_E2E=PASS' "$tmp/output.log" >/dev/null
test "$(grep -c 'probe.py' "$SSH_CALL_LOG")" = 3
test "$(grep -c 'probe.py.*65536' "$SSH_CALL_LOG")" = 2
test "$(grep -c 'probe.py.*2097289' "$SSH_CALL_LOG")" = 1
test "$(grep -c 'check-ports.py' "$SSH_CALL_LOG")" = 2
test "$(grep -c 'serve --listen' "$SSH_CALL_LOG")" = 4
test "$(grep -c 'forward --listen' "$SSH_CALL_LOG")" = 3
test "$(grep -c 'nohup python3' "$SSH_CALL_LOG")" = 2
test "$(grep -c -- '--local-id' "$SSH_CALL_LOG")" -ge 5
test "$(grep -c -- '--peer-id' "$SSH_CALL_LOG")" -ge 5
test "$(grep -c -- '--tunnel-id' "$SSH_CALL_LOG")" -ge 5
test "$(grep -c -- '--peer-fingerprint' "$SSH_CALL_LOG")" -ge 5
test "$(grep -c -- '--direction' "$SSH_CALL_LOG")" -ge 7
test "$(grep -c -- '--crl' "$SSH_CALL_LOG")" = 7
test "$(grep -c "grep -qi 'revok'" "$SSH_CALL_LOG")" = 2
test "$(grep -c -- '--direction.*direct' "$SSH_CALL_LOG")" = 7
test "$(grep -c -- '--direction.*reverse' "$SSH_CALL_LOG")" = 0
test "$(grep -E -c -- '--local_id|--peer_id|--tunnel_id|--peer_fingerprint' "$SSH_CALL_LOG" || true)" = 0
test "$(wc -l < "$SCP_CALL_LOG" | tr -d ' ')" = 6
test "$(grep -c 'ca.crl' "$SCP_CALL_LOG")" = 6
if grep -Eqi 'iptables|nft |systemctl|sysctl|tc qdisc|reboot' "$SSH_CALL_LOG"; then
  echo 'E2E orchestration unexpectedly attempted host-level mutation' >&2
  exit 1
fi
grep -Fx 'StrictHostKeyChecking=yes' "$SSH_POLICY_LOG" >/dev/null
grep -Fx "UserKnownHostsFile=$SSH_KNOWN_HOSTS_FILE" "$SSH_POLICY_LOG" >/dev/null
grep -Fx 'BatchMode=yes' "$SSH_POLICY_LOG" >/dev/null


# Reverse stage (LAB_REVERSE=1): configs go through `apply`, the private side never gets a listener.
: > "$SSH_CALL_LOG"; : > "$SCP_CALL_LOG"
LAB_REVERSE=1 bash "$root/lab/real-hosts/phase1-two-host-e2e.sh" > "$tmp/output-reverse.log"
grep -F 'REVERSE_RESULT engine=tcp-tls' "$tmp/output-reverse.log" >/dev/null
grep -F 'reverse=1' "$tmp/output-reverse.log" >/dev/null
test "$(grep -c 'rev-public.json' "$SSH_CALL_LOG")" -ge 4
test "$(grep -c 'rev-service.json' "$SSH_CALL_LOG")" -ge 6
test "$(grep -c '"ingress"' "$SSH_CALL_LOG")" -ge 3
# The service-node (forward) config never carries a listener, the public node never a target.
if grep 'rev-service.json' "$SSH_CALL_LOG" | grep -q '"listen"'; then echo 'service node must not listen' >&2; exit 1; fi
if grep 'rev-public.json' "$SSH_CALL_LOG" | grep -q '"target"'; then echo 'public node must not have a target' >&2; exit 1; fi
test "$(grep -c "grep -qi 'revok'" "$SSH_CALL_LOG")" = 3
if grep -Eqi 'iptables|nft |systemctl|sysctl|tc qdisc|reboot' "$SSH_CALL_LOG"; then
  echo 'Reverse stage unexpectedly attempted host-level mutation' >&2
  exit 1
fi

echo 'Phase 1 two-host E2E orchestration tests: PASS (mock SSH/SCP only; no network connections made)'
