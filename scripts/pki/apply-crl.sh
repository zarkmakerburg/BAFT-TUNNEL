#!/usr/bin/env bash
# Validate and atomically activate an offline-issued CRL for one local instance.
set -Eeuo pipefail
umask 077

usage() {
  cat <<'USAGE'
Usage:
  apply-crl.sh check CA_CERT CURRENT_CRL CANDIDATE_CRL
  apply-crl.sh apply CA_CERT CURRENT_CRL CANDIDATE_CRL INSTANCE

check validates signature, CRL number monotonicity, and candidate validity
without changing files or services. apply additionally requires root/systemd,
atomically replaces CURRENT_CRL, restarts baft-tunnel@INSTANCE.service, and
restores the previous CRL if restart/active-state verification fails.
USAGE
}
die() { echo "apply-crl: $*" >&2; exit 2; }

[[ $# -ge 1 ]] || { usage; exit 2; }
ACTION="$1"; shift
case "$ACTION" in
  check) [[ $# == 3 ]] || { usage; exit 2; } ;;
  apply) [[ $# == 4 ]] || { usage; exit 2; } ;;
  *) usage; die "unknown action: $ACTION" ;;
esac
CA_CERT="$1"; CURRENT_CRL="$2"; CANDIDATE_CRL="$3"
INSTANCE="${4:-}"

for file in "$CA_CERT" "$CURRENT_CRL" "$CANDIDATE_CRL"; do
  [[ -f "$file" && ! -L "$file" && -r "$file" ]] || die "expected readable regular non-symlink file: $file"
done
if [[ "$ACTION" == "apply" ]]; then
  [[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_.@-]{0,62}$ ]] || die "invalid systemd instance name"
  [[ "$INSTANCE" != *..* ]] || die "invalid systemd instance name"
  [[ "$(id -u)" == 0 ]] || die "apply requires root"
  command -v systemctl >/dev/null 2>&1 || die "systemctl is required"
  command -v openssl >/dev/null 2>&1 || die "openssl is required"
  command -v python3 >/dev/null 2>&1 || die "python3 is required"
fi

# OpenSSL verifies the CRL signature against the configured trust anchor.
# Python parses OpenSSL's stable machine-oriented fields and checks the time window.
python3 - "$CA_CERT" "$CURRENT_CRL" "$CANDIDATE_CRL" <<'PY'
import datetime
import re
import subprocess
import sys

ca, current, candidate = sys.argv[1:]

def strict_pem(path):
    with open(path, "rb") as stream:
        data = stream.read(16 * 1024 * 1024 + 1)
    if len(data) > 16 * 1024 * 1024:
        raise ValueError("CRL file exceeds 16 MiB limit")
    pattern = rb"\A\s*-----BEGIN X509 CRL-----\s*[A-Za-z0-9+/=\r\n]+-----END X509 CRL-----\s*\Z"
    if not re.fullmatch(pattern, data):
        raise ValueError("file must contain exactly one PEM X509 CRL and no trailing data")

def openssl(path, *args):
    return subprocess.check_output(
        ["openssl", "crl", "-in", path, "-noout", *args],
        text=True, stderr=subprocess.PIPE,
    ).strip()

def verify(path):
    strict_pem(path)
    subprocess.run(
        ["openssl", "crl", "-in", path, "-noout", "-verify", "-CAfile", ca],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
    )
    raw = openssl(path, "-crlnumber")
    match = re.fullmatch(r"crlNumber=0x([0-9A-Fa-f]+)", raw)
    if not match:
        raise ValueError("CRL Number extension is missing or malformed")
    ca_subject = subprocess.check_output(
        ["openssl", "x509", "-in", ca, "-noout", "-subject", "-nameopt", "RFC2253"],
        text=True, stderr=subprocess.PIPE,
    ).strip().removeprefix("subject=")
    crl_issuer = subprocess.check_output(
        ["openssl", "crl", "-in", path, "-noout", "-issuer", "-nameopt", "RFC2253"],
        text=True, stderr=subprocess.PIPE,
    ).strip().removeprefix("issuer=")
    if crl_issuer != ca_subject:
        raise ValueError("CRL issuer does not match the configured CA subject")
    return int(match.group(1), 16)

def parse_time(value):
    match = re.fullmatch(r"(?:lastUpdate|nextUpdate)=(.+) GMT", value)
    if not match:
        raise ValueError("CRL validity timestamp is missing or malformed")
    return datetime.datetime.strptime(match.group(1), "%b %d %H:%M:%S %Y").replace(tzinfo=datetime.timezone.utc)

try:
    old_number = verify(current)
    new_number = verify(candidate)
    if new_number <= old_number:
        raise ValueError(f"candidate CRL number {new_number} is not greater than installed number {old_number}")
    now = datetime.datetime.now(datetime.timezone.utc)
    last = parse_time(openssl(candidate, "-lastupdate"))
    nxt = parse_time(openssl(candidate, "-nextupdate"))
    if last > now + datetime.timedelta(seconds=60):
        raise ValueError("candidate CRL is not yet valid (more than 60 seconds in the future)")
    if nxt <= now:
        raise ValueError("candidate CRL has expired")
    fingerprint = openssl(candidate, "-fingerprint", "-sha256").split("=", 1)[-1]
    print(f"CRL_CHECK=PASS old_number={old_number} candidate_number={new_number} sha256={fingerprint} next_update={nxt.isoformat()}")
except (subprocess.CalledProcessError, OSError, ValueError) as exc:
    print(f"apply-crl: CRL validation failed: {exc}", file=sys.stderr)
    sys.exit(2)
PY

if [[ "$ACTION" == "check" ]]; then
  exit 0
fi

SERVICE="baft-tunnel@${INSTANCE}.service"
systemctl is-active --quiet "$SERVICE" || die "selected service must already be active; refusing to start or enable it"
CRL_DIR="$(cd "$(dirname "$CURRENT_CRL")" && pwd -P)"
CRL_NAME="$(basename "$CURRENT_CRL")"
CURRENT_CRL="$CRL_DIR/$CRL_NAME"
[[ -d "$CRL_DIR" && -w "$CRL_DIR" ]] || die "CRL directory is not writable: $CRL_DIR"
BACKUP="$(mktemp "$CRL_DIR/.${CRL_NAME}.rollback.XXXXXX")"
STAGED="$(mktemp "$CRL_DIR/.${CRL_NAME}.staged.XXXXXX")"
INSTALLED=0
cleanup() {
  local status=$?
  rm -f "$STAGED"
  if [[ "$INSTALLED" == 0 ]]; then rm -f "$BACKUP"; fi
  exit "$status"
}
trap cleanup EXIT

cp -p "$CURRENT_CRL" "$BACKUP"
install -o root -g root -m 0644 "$CANDIDATE_CRL" "$STAGED"
mv -f "$STAGED" "$CURRENT_CRL"
activation_status=0
systemctl restart "$SERVICE" || activation_status=$?
if [[ "$activation_status" == 0 ]]; then
  systemctl is-active --quiet "$SERVICE" || activation_status=$?
fi
if [[ "$activation_status" == 0 ]]; then
  INSTALLED=1
  rm -f "$BACKUP"
  echo "CRL_APPLY=PASS service=$SERVICE"
  exit 0
fi

echo "apply-crl: service restart/active check failed; restoring previous CRL" >&2
INSTALLED=2
mv -f "$BACKUP" "$CURRENT_CRL"
INSTALLED=1
if systemctl restart "$SERVICE" && systemctl is-active --quiet "$SERVICE"; then
  INSTALLED=1
  echo "CRL_APPLY=ROLLED_BACK service=$SERVICE activation_status=$activation_status" >&2
  exit 1
fi
echo "CRL_APPLY=ROLLBACK_FAILED service=$SERVICE; inspect service and CRL immediately" >&2
INSTALLED=2
exit 3
