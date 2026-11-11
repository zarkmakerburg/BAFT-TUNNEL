#!/usr/bin/env bash
# Offline tests for prepare-ops-package.sh; no network or host access.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
HELPER="$ROOT/lab/real-hosts/prepare-ops-package.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-ops-package-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/src/scripts/pki" "$TMP/src/systemd"
printf '#!/bin/sh\necho test\n' > "$TMP/src/baft-tunnel"
chmod 0755 "$TMP/src/baft-tunnel"
for f in install update backup restore uninstall install-agent-control; do printf '#!/bin/sh\nexit 0\n' > "$TMP/src/scripts/$f.sh"; chmod 0755 "$TMP/src/scripts/$f.sh"; done
printf '#!/bin/sh\nexit 0\n' > "$TMP/src/scripts/pki/apply-crl.sh"; chmod 0755 "$TMP/src/scripts/pki/apply-crl.sh"
printf '[Service]\nExecStart=/usr/local/bin/baft-tunnel\n' > "$TMP/src/systemd/baft-tunnel@.service"
mkdir "$TMP/src-root"; mv "$TMP/src" "$TMP/src-root/baft-tunnel-test-linux-x86_64"
tar -czf "$TMP/good.tgz" -C "$TMP/src-root" baft-tunnel-test-linux-x86_64
cp "$TMP/src-root/baft-tunnel-test-linux-x86_64/baft-tunnel" "$TMP/expected"

# Valid package is extracted and its executable is byte-identical.
root="$("$HELPER" "$TMP/good.tgz" "$TMP/expected" "$TMP/out")"
cmp -s "$TMP/expected" "$root/baft-tunnel"

# A mismatched separately selected binary is refused.
printf 'not the package binary\n' > "$TMP/mismatch"; chmod 0755 "$TMP/mismatch"
if "$HELPER" "$TMP/good.tgz" "$TMP/mismatch" "$TMP/out-mismatch" >/dev/null 2>&1; then echo 'accepted mismatched binary' >&2; exit 1; fi

# A symlink entry is refused.
mkdir -p "$TMP/link-root/pkg/scripts/pki" "$TMP/link-root/pkg/systemd"
ln -s /etc/passwd "$TMP/link-root/pkg/evil"
tar -czf "$TMP/link.tgz" -C "$TMP/link-root" pkg
if "$HELPER" "$TMP/link.tgz" "$TMP/expected" "$TMP/out-link" >/dev/null 2>&1; then echo 'accepted symlink entry' >&2; exit 1; fi

# A traversal entry is refused.
python3 - "$TMP/traversal.tgz" <<'PY'
import io, sys, tarfile
with tarfile.open(sys.argv[1], 'w:gz') as tf:
    data=b'owned'
    info=tarfile.TarInfo('pkg/../../escape')
    info.size=len(data)
    tf.addfile(info, io.BytesIO(data))
PY
if "$HELPER" "$TMP/traversal.tgz" "$TMP/expected" "$TMP/out-traversal" >/dev/null 2>&1; then echo 'accepted traversal entry' >&2; exit 1; fi

echo 'prepare-ops-package tests: PASS (safe paths, regular files, required package layout, binary identity)'
