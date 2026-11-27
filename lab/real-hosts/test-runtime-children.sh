#!/usr/bin/env bash
# Regression test for the runtime-directory allowlist (no root, no real host).
#
# The 2026-11-27 two-host run completed pairing, 64 KB and ~2 MB integrity, then refused cleanup on
# both hosts because /run/baft-tunnel/manage — the product's own control-manage socket directory,
# declared by tmpfiles.d — was not in the allowlist. These cases hold the allowlist to exactly the
# product's two directories and prove that an unknown or unsafe child still stops the cleanup.
set -Eeuo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
# shellcheck source=lab/real-hosts/lib-runtime-children.sh
source "$root/lab/real-hosts/lib-runtime-children.sh"

fail() { echo "runtime-children contract: $*" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
# Production calls the check with its root:root default; the test runs unprivileged, so positive
# cases name the current owner and the default is still proven to refuse a foreign owner.
owner="$(id -un):$(id -gn)"
parent="$tmp/run/baft-tunnel"
mkdir -p "$parent/control" "$parent/manage"

baft_runtime_children_known "$parent" "$parent/control" "$parent/manage" ||
  fail "control and manage must both be accepted"
baft_manage_dir_known "$parent/manage" "$owner" ||
  fail "an empty manage directory must be accepted"
if [[ "$owner" != root:root ]]; then
  if baft_manage_dir_known "$parent/manage" 2>"$tmp/err"; then
    fail "the root:root default must refuse a directory with another owner"
  fi
fi

# A live control-manage unit leaves one root-owned socket per instance.
python3 - "$parent/manage" <<'PY'
import os
import socket
import sys

path = os.path.join(sys.argv[1], "1000.manage.sock")
sock = socket.socket(socket.AF_UNIX)
sock.bind(path)
sock.close()
assert os.path.exists(path)
PY
baft_manage_dir_known "$parent/manage" "$owner" ||
  fail "a *.manage.sock socket must be accepted"

# A socket with another name is not one of the product's manage sockets.
python3 - "$parent/manage" <<'PY2'
import os
import socket
import sys

path = os.path.join(sys.argv[1], "other.sock")
sock = socket.socket(socket.AF_UNIX)
sock.bind(path)
sock.close()
assert os.path.exists(path)
PY2
if baft_manage_dir_known "$parent/manage" "$owner" 2>"$tmp/err"; then
  fail "a socket without the manage name must be refused"
fi
grep -Fq "unexpected manage socket name" "$tmp/err" ||
  fail "the refusal must name the unexpected socket"
rm -f "$parent/manage/other.sock"

# A child outside the allowlist is refused and named — never removed.
if baft_runtime_children_known "$parent" "$parent/control" 2>"$tmp/err"; then
  fail "manage must be refused when the caller does not allow it"
fi
grep -Fq "$parent/manage; preserving it" "$tmp/err" ||
  fail "the refusal must name the unexpected path"

mkdir "$parent/unknown"
if baft_runtime_children_known "$parent" "$parent/control" "$parent/manage" 2>"$tmp/err"; then
  fail "an unknown runtime child must be refused"
fi
grep -Fq "$parent/unknown; preserving it" "$tmp/err" ||
  fail "the refusal must name the unknown path"
rmdir "$parent/unknown"

# A regular file inside manage is not a socket and is refused.
printf 'notes\n' >"$parent/manage/notes"
if baft_manage_dir_known "$parent/manage" "$owner" 2>"$tmp/err"; then
  fail "a regular file in manage must be refused"
fi
grep -Fq "unexpected manage runtime entry" "$tmp/err" ||
  fail "the refusal must name the file"
rm -f "$parent/manage/notes"

# A symlink in manage is refused even when its name looks like a socket.
ln -s /tmp "$parent/manage/1001.manage.sock"
if baft_manage_dir_known "$parent/manage" "$owner" 2>"$tmp/err"; then
  fail "a symlink in manage must be refused"
fi
rm -f "$parent/manage/1001.manage.sock"

# A manage path that is itself a symlink is refused.
rm -f "$parent/manage/1000.manage.sock"
rmdir "$parent/manage"
ln -s /tmp "$parent/manage"
if baft_manage_dir_known "$parent/manage" "$owner" 2>"$tmp/err"; then
  fail "a symlinked manage directory must be refused"
fi
rm -f "$parent/manage"

# A missing runtime parent is unsafe, not "clean".
if baft_runtime_children_known "$tmp/missing" "$tmp/missing/control" 2>"$tmp/err"; then
  fail "a missing runtime parent must be refused"
fi

echo "runtime children contract: PASS"
