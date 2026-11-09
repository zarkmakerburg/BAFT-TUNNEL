#!/usr/bin/env bash
# Staged test of the interactive menu (scripts/menu.sh) with the real binary:
#   test-menu.sh PATH_TO_BAFT_TUNNEL_BINARY
# Plain mode (numbered input on stdin) drives install + register, pairing (init on two staged
# nodes, join with a checksum typed in Persian digits and spaces), config plan, service start
# (skipped in staging), backup/restore, rollback and uninstall. A pseudo-terminal run checks the
# arrow keys, Enter, Esc and q. Nothing outside the temporary directories is touched.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
BIN_SRC="${1:?usage: test-menu.sh BAFT_TUNNEL_BINARY}"
BIN_SRC="$(cd "$(dirname "$BIN_SRC")" && pwd -P)/$(basename "$BIN_SRC")"
MENU="$ROOT/scripts/menu.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-menu-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
fail() { echo "FAIL: $*" >&2; [[ ! -f "$TMP/out" ]] || { echo "--- last output:" >&2; tail -n 40 "$TMP/out" >&2; }; exit 1; }
# menu ROOT INPUT [ARGS...]: plain mode, English, output in $TMP/out
menu() {
  local stage="$1" input="$2"; shift 2
  printf '%b' "$input" | bash "$MENU" --plain --lang en --destdir "$stage" "$@" > "$TMP/out" 2>&1 || fail "menu exited non-zero"
}
has() { grep -Fq -- "$1" "$TMP/out" || fail "expected: $1"; }
hasnt() { ! grep -Fq -- "$1" "$TMP/out" || fail "unexpected: $1"; }

# As root, `setup init/join --instance` hands files to the real service user; staging runs unprivileged.
[[ "$(id -u)" != "0" ]] || { echo "test-menu: run as an unprivileged user" >&2; exit 2; }
bash -n "$MENU"
if command -v shellcheck >/dev/null 2>&1; then shellcheck -S warning "$MENU" "$0"; fi

# Banner and help.
bash "$MENU" --banner > "$TMP/out"; has '|____/_/   \_\_|'
COLUMNS=40 bash "$MENU" --banner > "$TMP/out"; has 'T U N N E L'

# Persian menu text in plain mode, then Exit (12).
menu "$TMP/fa" '12\n' --lang fa
has 'منوی اصلی'; has 'جفت‌کردن دو نود'; has 'خداحافظ'

# 1) Install + register on node A (abroad) and node B (iran).
A="$TMP/a"; B="$TMP/b"
for stage in "$A" "$B"; do
  menu "$stage" "1\n$BIN_SRC\nmain\n12\n"
  has 'install.sh --binary'; has 'Installed '; has 'Next step:'; has 'Pair the two nodes'
  has 'Registered instance main'; has 'Bye.'
  [[ -x "$stage/usr/local/bin/baft-tunnel" ]] || fail "binary not installed"
  [[ -f "$stage/usr/local/lib/baft-tunnel/scripts/menu.sh" ]] || fail "menu not installed in lib"
  [[ -f "$stage/usr/local/lib/baft-tunnel/systemd/baft-tunnel@.service" ]] || fail "units not installed in lib"
  [[ ! -e "$stage/usr/local/lib/baft-tunnel/scripts/test-menu.sh" ]] || fail "tests must not be installed"
done
# Second run offers the guarded upgrade; 0 goes back without changes.
menu "$A" "1\n0\n12\n"; has 'An installation exists'

# 2) Pairing: init on A (serve) and B (forward), join on B with A's bundle.
menu "$A" "3\n1\n2\n1\nabroad\niran\nmain\n18080\n\n12\n"
has 'setup init --engine noise-tcp --role serve'; has 'Checksum (compare'; has 'scp '
SUM_A="$(sed -n 's/^Checksum (compare with the other node out of band): //p' "$TMP/out")"
[[ "$SUM_A" =~ ^[0-9a-f]{8}(-[0-9a-f]{8}){3}$ ]] || fail "no checksum from init"
menu "$B" "3\n1\n1\n1\niran\nabroad\nmain\n18080\n\n203.0.113.10\n12\n"
has '--role forward'; has '--peer-host 203.0.113.10'

# A wrong checksum is refused with one clean line and no backtrace, even with RUST_BACKTRACE.
RUST_BACKTRACE=1 menu "$B" "3\n2\n$A/etc/baft-tunnel/bundle.json\n00000000-00000000-00000000-00000000\n2\n12\n"
has 'Error: bundle checksum does not match'; hasnt 'backtrace'; hasnt 'Stack'; has 'Failed (exit 1)'
# A checksum that is not 32 hex characters is asked again; "No" at the comparison cancels.
menu "$B" "3\n2\n\nxyz\n${SUM_A}\n1\n12\n"
has 'A checksum is 32 hex characters'; has 'Cancelled.'
# Persian digits, upper case and spaces are accepted.
FA_SUM="$(printf '%s' "${SUM_A^^}" | tr '-' ' ' | sed 's/0/۰/g; s/1/۱/g; s/2/۲/g; s/3/۳/g; s/4/۴/g; s/5/۵/g; s/6/۶/g; s/7/۷/g; s/8/۸/g; s/9/۹/g')"
menu "$B" "3\n2\n$A/etc/baft-tunnel/bundle.json\n$FA_SUM\n2\n12\n"
has "You typed: $SUM_A"; has 'Configuration written:'; has 'Configuration > Plan'
[[ -f "$B/etc/baft-tunnel/main.json" ]] || fail "join did not write main.json"

# 3) Config plan for noise-tcp: no blank TLS line, no LAB wording.
menu "$B" "4\n1\n12\n"
has 'Mode: forward'; has 'Noise static key file'; hasnt 'TLS server name'; hasnt 'LAB compatibility'
menu "$B" "4\n2\n12\n"; has 'Configuration valid.'

# 4) Tunnel start/stop/restart: systemd steps are skipped in staging, after a confirmation.
menu "$B" "5\n1\n2\n12\n"; has 'Skipped in staging mode (needs systemd): systemctl enable --now baft-tunnel@main.service'
menu "$B" "5\n2\n1\n12\n"; has 'Cancelled.'; hasnt 'disable --now'
menu "$B" "6\n1\n12\n"; has 'systemctl --no-pager --lines=0 status baft-tunnel@main.service'

# 5) Backup, then restore it (typed confirmation).
menu "$B" "8\n1\n12\n"; has 'backup.sh --destdir'
menu "$B" "8\n2\n1\nno\n12\n"; has 'Cancelled.'
menu "$B" "8\n2\n1\nyes\n12\n"; has 'restore.sh --destdir'; has 'menu > Tunnel > Start'

# 6) Roll back the last install: next step is printed, lib restored or removed with the binary.
menu "$A" "9\n2\n1\n2\n12\n"
has 'rollback.sh --destdir'; has 'Rollback complete'; has 'Next step:'; has 'Tunnel > Restart'

# 7) Uninstall keeps configuration and keys by default.
menu "$B" "10\n2\n1\n12\n"
has 'uninstall.sh --destdir'; has 'were kept in'
[[ ! -e "$B/usr/local/bin/baft-tunnel" && ! -e "$B/usr/local/lib/baft-tunnel" ]] || fail "uninstall left binary or lib"
[[ -f "$B/etc/baft-tunnel/main.json" ]] || fail "uninstall removed config"

# 8) Dry run prints and changes nothing.
menu "$TMP/dry" "1\n$BIN_SRC\n\n12\n" --dry-run
has 'Would run:'; [[ ! -e "$TMP/dry/usr/local/bin/baft-tunnel" ]] || fail "dry run installed"

# 9) Arrow keys, Enter, Esc and q on a real pseudo-terminal.
if command -v script >/dev/null 2>&1; then
  { printf '\033[B\033[B\r'; sleep 0.5; printf '\033'; sleep 0.5; printf 'q'; sleep 0.5; } |
    timeout 20 script -qec "bash '$MENU' --lang en --destdir '$TMP/tty'" /dev/null > "$TMP/out" 2>&1 ||
    fail "pty run failed"
  has '|____/_/   \_\_|'; has ' > Pair the two nodes (init / join) '; has 'This node first'; has 'Bye.'
  has 'Up/Down: move'
fi
echo "BAFT_MENU_TEST=PASS (banner, fa/en, install+register+next step, init/join with checksum compare, clean join error, config plan, staged service actions, backup/restore, rollback next step, uninstall, dry run, arrow/Enter/Esc/q on a pty)"
