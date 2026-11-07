#!/usr/bin/env bash
# P1-000 isolated LAB test: run the baseline LoadCredential= lifecycle test on one
# LAB host. The test refuses to run if any BAFT-TUNNEL install already exists,
# creates only disposable paths, and removes everything it created on exit.
# Usage: p1-000-credential-lab.sh LABEL HOST USER PORT BINARY
set -Eeuo pipefail
umask 077
(($# == 5)) || { echo 'usage: p1-000-credential-lab.sh LABEL HOST USER PORT BINARY' >&2; exit 2; }
LABEL="$1" HOST="$2" USER_NAME="$3" PORT="$4" BINARY="$5"
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
[[ -x "$BINARY" ]] || { echo 'binary missing' >&2; exit 2; }
[[ -f "${SSH_KEY_FILE:-}" && -f "${SSH_KNOWN_HOSTS_FILE:-}" ]] || { echo 'ssh files missing' >&2; exit 2; }
SSH_OPTS=(-i "$SSH_KEY_FILE" -p "$PORT" -o BatchMode=yes -o IdentitiesOnly=yes
  -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE"
  -o ConnectTimeout=12 -o ServerAliveInterval=10 -o ServerAliveCountMax=2 -o LogLevel=ERROR)
SCP_OPTS=(-i "$SSH_KEY_FILE" -P "$PORT" -o BatchMode=yes -o IdentitiesOnly=yes
  -o StrictHostKeyChecking=yes -o UserKnownHostsFile="$SSH_KNOWN_HOSTS_FILE" -o LogLevel=ERROR)
REMOTE_DIR="/tmp/baft-p1000-$RANDOM$RANDOM"
BUNDLE="$(mktemp -d)/bundle.tar"
tar -C "$ROOT" -cf "$BUNDLE" scripts/install.sh scripts/uninstall.sh scripts/rollback.sh \
  scripts/pki/test-systemd-baseline-lifecycle.sh systemd/baft-tunnel@.service
echo "=== P1-000 LAB credential test: $LABEL ==="
cleanup() {
  ssh "${SSH_OPTS[@]}" "$USER_NAME@$HOST" "rm -rf '$REMOTE_DIR'" >/dev/null 2>&1 || true
  rm -rf "$(dirname "$BUNDLE")"
}
trap cleanup EXIT
ssh "${SSH_OPTS[@]}" "$USER_NAME@$HOST" "mkdir -m 0700 '$REMOTE_DIR' && . /etc/os-release && echo \"os=\$PRETTY_NAME\" && systemctl --version | head -n 1"
scp "${SCP_OPTS[@]}" -q "$BUNDLE" "$BINARY" "$USER_NAME@$HOST:$REMOTE_DIR/"
ssh "${SSH_OPTS[@]}" "$USER_NAME@$HOST" "cd '$REMOTE_DIR' && tar -xf bundle.tar && chmod +x baft-tunnel && bash scripts/pki/test-systemd-baseline-lifecycle.sh '$REMOTE_DIR/baft-tunnel'"
echo "=== P1-000 LAB credential test PASS: $LABEL ==="
