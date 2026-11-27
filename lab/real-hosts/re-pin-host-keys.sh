#!/usr/bin/env bash
# Re-pin the LAB hosts' ED25519 host keys after an address change.
#
# Operator-run and local only. It never contacts a host for anything but the public
# host key, never writes repository files, and never weakens host-key checking:
# every real-host workflow still runs with StrictHostKeyChecking=yes against the
# pinned GitHub Environment secret LAB_SSH_KNOWN_HOSTS.
#
# It prints only fingerprints. The known_hosts lines go to a mode-0600 file the
# operator pastes into the Environment secret; pass --print-known-hosts only when
# you are pasting locally, never in a recorded or uploaded terminal.
set -Eeuo pipefail
umask 077

die() { echo "host-key re-pin: $*" >&2; exit 2; }

usage() {
  cat >&2 <<'USAGE'
usage: re-pin-host-keys.sh --iran HOST [--iran-port N] --abroad HOST [--abroad-port N]
                           [--out FILE] [--print-known-hosts]
                           [--expect-iran SHA256:...] [--expect-abroad SHA256:...]

  --iran / --abroad            LAB host address (as configured in the Environment secret)
  --iran-port / --abroad-port  SSH port, default 22
  --out FILE                   write the pinned known_hosts here (default ./lab-known_hosts.pinned)
  --print-known-hosts          also print the known_hosts lines for pasting into the secret
  --expect-*                   abort unless the scanned fingerprint matches this value
                               (copy it from the provider console; this is the independent check)

Update GitHub: Settings -> Environments -> baft-tunnel-lab -> secret LAB_SSH_KNOWN_HOSTS.
USAGE
  exit 2
}

[[ -z "${GITHUB_ACTIONS:-}" ]] ||
  die 'refusing to run inside GitHub Actions: host-key material must never reach CI logs or artifacts'

IRAN_HOST='' ABROAD_HOST='' IRAN_PORT=22 ABROAD_PORT=22
OUT='' PRINT_KNOWN_HOSTS=0 EXPECT_IRAN='' EXPECT_ABROAD=''

while (($#)); do
  case "$1" in
    --iran) (($# >= 2)) || usage; IRAN_HOST="$2"; shift 2 ;;
    --abroad) (($# >= 2)) || usage; ABROAD_HOST="$2"; shift 2 ;;
    --iran-port) (($# >= 2)) || usage; IRAN_PORT="$2"; shift 2 ;;
    --abroad-port) (($# >= 2)) || usage; ABROAD_PORT="$2"; shift 2 ;;
    --out) (($# >= 2)) || usage; OUT="$2"; shift 2 ;;
    --print-known-hosts) PRINT_KNOWN_HOSTS=1; shift ;;
    --expect-iran) (($# >= 2)) || usage; EXPECT_IRAN="$2"; shift 2 ;;
    --expect-abroad) (($# >= 2)) || usage; EXPECT_ABROAD="$2"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "host-key re-pin: unexpected argument: $1" >&2; usage ;;
  esac
done

[[ -n "$IRAN_HOST" ]] || usage
[[ -n "$ABROAD_HOST" ]] || usage
[[ -n "$OUT" ]] || OUT='./lab-known_hosts.pinned'

validate_host() {
  local label="$1" value="$2"
  if [[ "$value" == *:* ]]; then
    python3 -c 'import ipaddress,sys; ipaddress.IPv6Address(sys.argv[1])' "$value" >/dev/null 2>&1 ||
      die "invalid IPv6 address for $label"
  else
    [[ "$value" =~ ^[A-Za-z0-9][A-Za-z0-9.-]*$ ]] || die "invalid host value for $label"
  fi
}
validate_host --iran "$IRAN_HOST"
validate_host --abroad "$ABROAD_HOST"
[[ "$IRAN_HOST" != "$ABROAD_HOST" ]] || die 'the two LAB hosts must differ'
for port in "$IRAN_PORT" "$ABROAD_PORT"; do
  [[ "$port" =~ ^[0-9]+$ ]] && ((10#$port >= 1 && 10#$port <= 65535)) || die 'invalid SSH port'
done
for tool in ssh-keyscan ssh-keygen; do
  command -v "$tool" >/dev/null || die "missing local dependency: $tool"
done

# scan SIDE HOST PORT: exactly one distinct ED25519 host key, printed as "<type> <blob>".
scan() {
  local side="$1" host="$2" port="$3" raw keys count
  raw="$(ssh-keyscan -T 10 -p "$port" -t ed25519 "$host" 2>/dev/null)" ||
    die "ssh-keyscan failed for $side; the address is unreachable or refused the key scan"
  keys="$(printf '%s\n' "$raw" | awk 'NF >= 3 && $2 == "ssh-ed25519" { print $2 " " $3 }' | sort -u)"
  count="$(printf '%s\n' "$keys" | grep -c . || true)"
  ((count > 0)) || die "$side presented no ED25519 host key"
  ((count == 1)) || die "$side presented $count distinct ED25519 host keys (expected exactly one)"
  printf '%s\n' "$keys"
}

fingerprint() { # "<type> <blob>" -> "SHA256:..."
  printf '%s\n' "$1" | ssh-keygen -lf - 2>/dev/null | awk '{print $2}'
}

# known_hosts entry: bare host on port 22, "[host]:port" otherwise.
known_hosts_line() {
  local host="$1" port="$2" key="$3"
  if ((port == 22)); then
    printf '%s %s\n' "$host" "$key"
  else
    printf '[%s]:%s %s\n' "$host" "$port" "$key"
  fi
}

check_expected() {
  local side="$1" expected="$2" actual="$3"
  [[ -n "$expected" ]] || return 0
  [[ "$expected" == "$actual" ]] ||
    die "$side fingerprint mismatch: provider console says $expected, host presented $actual; do not pin this key"
}

IRAN_KEY="$(scan iran "$IRAN_HOST" "$IRAN_PORT")"
ABROAD_KEY="$(scan abroad "$ABROAD_HOST" "$ABROAD_PORT")"
IRAN_FP="$(fingerprint "$IRAN_KEY")"
ABROAD_FP="$(fingerprint "$ABROAD_KEY")"
[[ "$IRAN_FP" == SHA256:* ]] || die 'could not compute the Iran host-key fingerprint'
[[ "$ABROAD_FP" == SHA256:* ]] || die 'could not compute the abroad host-key fingerprint'
check_expected iran "$EXPECT_IRAN" "$IRAN_FP"
check_expected abroad "$EXPECT_ABROAD" "$ABROAD_FP"
[[ "$IRAN_FP" != "$ABROAD_FP" ]] || die 'both hosts presented the same host key; refuse to pin it'

tmp="$(mktemp)" || die 'could not create a temporary file'
trap 'rm -f "$tmp"' EXIT HUP INT TERM
{
  known_hosts_line "$IRAN_HOST" "$IRAN_PORT" "$IRAN_KEY"
  known_hosts_line "$ABROAD_HOST" "$ABROAD_PORT" "$ABROAD_KEY"
} > "$tmp"
[[ "$(grep -c . "$tmp" || true)" == 2 ]] || die 'expected exactly two known_hosts entries'
chmod 600 "$tmp"
if [[ -e "$OUT" ]]; then
  [[ -f "$OUT" && ! -L "$OUT" ]] || die "refusing to overwrite an unsafe path: $OUT"
fi
cp -- "$tmp" "$OUT"
chmod 600 "$OUT"

echo "iran_host_key_fingerprint=$IRAN_FP"
echo "abroad_host_key_fingerprint=$ABROAD_FP"
echo "known_hosts_file=$OUT entries=2 mode=$(stat -c '%a' "$OUT")"
if ((PRINT_KNOWN_HOSTS == 1)); then
  echo '--- paste into secret LAB_SSH_KNOWN_HOSTS (baft-tunnel-lab) ---'
  cat "$OUT"
  echo '--- end of secret value ---'
else
  echo 'known_hosts lines withheld; pass --print-known-hosts to print them for the paste step'
fi
cat <<'NEXT'

Next steps (owner):
1. Compare both fingerprints with the provider console before pinning; a mismatch means
   the address now points at a different machine.
2. Settings -> Environments -> baft-tunnel-lab -> LAB_SSH_KNOWN_HOSTS -> paste the file.
3. Re-run "Phase 0 two-host LAB (manual, read-only preflight)" with READ_ONLY_PREFLIGHT.
NEXT
