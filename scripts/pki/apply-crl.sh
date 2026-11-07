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
  command -v flock >/dev/null 2>&1 || die "flock is required"
  command -v openssl >/dev/null 2>&1 || die "openssl is required"
  command -v python3 >/dev/null 2>&1 || die "python3 is required"
fi

# OpenSSL verifies each CRL signature against the CA whose subject equals the CRL issuer.
# CA_CERT may be a bundle of 1-4 CA certificates and each CRL file a bundle of the same CRLs
# (ADR-0017 B1 rollover overlap). Python parses OpenSSL's stable fields and checks time windows.
python3 - "$CA_CERT" "$CURRENT_CRL" "$CANDIDATE_CRL" <<'PY'
import datetime
import os
import re
import subprocess
import sys
import tempfile

ca_file, current, candidate = sys.argv[1:]
MAX_BUNDLE = 4

def read_blocks(path, label, kind):
    with open(path, "rb") as stream:
        data = stream.read(16 * 1024 * 1024 + 1)
    if len(data) > 16 * 1024 * 1024:
        raise ValueError(f"{label} exceeds 16 MiB limit")
    block = rb"-----BEGIN " + kind + rb"-----\s*[A-Za-z0-9+/=\r\n]+-----END " + kind + rb"-----"
    if not re.fullmatch(rb"\s*(?:" + block + rb"\s*)+", data):
        raise ValueError(f"{label} must contain only PEM {kind.decode()} blocks and no trailing data")
    blocks = re.findall(block, data)
    if not 1 <= len(blocks) <= MAX_BUNDLE:
        raise ValueError(f"{label} must contain 1-{MAX_BUNDLE} PEM blocks")
    return blocks

def run(args, data):
    return subprocess.run(args, input=data, check=True, capture_output=True).stdout.decode().strip()

def subject(cert):
    return run(["openssl", "x509", "-noout", "-subject", "-nameopt", "RFC2253"], cert).removeprefix("subject=")

def crl_field(crl, *flags):
    return run(["openssl", "crl", "-noout", *flags], crl)

def load_cas():
    cas = {}
    for cert in read_blocks(ca_file, "CA certificate file", b"CERTIFICATE"):
        name = subject(cert)
        if name in cas:
            raise ValueError(f"duplicate CA subject in bundle (CRLs are matched by subject): {name}")
        cas[name] = cert
    return cas

def load_crls(path, label, cas, require_ca):
    out = {}
    for crl in read_blocks(path, label, b"X509 CRL"):
        issuer = crl_field(crl, "-issuer", "-nameopt", "RFC2253").removeprefix("issuer=")
        if issuer in out:
            raise ValueError(f"{label} has two CRLs from the same issuer: {issuer}")
        ca = cas.get(issuer)
        if ca is None and require_ca:
            raise ValueError(f"{label} has a CRL whose issuer is not a configured CA: {issuer}")
        if ca is not None:
            with tempfile.NamedTemporaryFile("wb", delete=False) as handle:
                handle.write(ca)
            try:
                subprocess.run(["openssl", "crl", "-noout", "-verify", "-CAfile", handle.name],
                               input=crl, check=True, capture_output=True)
            except subprocess.CalledProcessError as exc:
                raise ValueError(f"CRL signature verification failed for issuer {issuer}") from exc
            finally:
                os.unlink(handle.name)
        raw = crl_field(crl, "-crlnumber")
        match = re.fullmatch(r"crlNumber=0x([0-9A-Fa-f]+)", raw)
        if not match:
            raise ValueError("CRL Number extension is missing or malformed")
        out[issuer] = (int(match.group(1), 16), crl)
    return out

def parse_time(value):
    match = re.fullmatch(r"(?:lastUpdate|nextUpdate)=(.+) GMT", value)
    if not match:
        raise ValueError("CRL validity timestamp is missing or malformed")
    return datetime.datetime.strptime(match.group(1), "%b %d %H:%M:%S %Y").replace(tzinfo=datetime.timezone.utc)

try:
    cas = load_cas()
    old = load_crls(current, "installed CRL file", cas, require_ca=False)
    new = load_crls(candidate, "candidate CRL file", cas, require_ca=True)
    missing = sorted(set(cas) - set(new))
    if missing:
        raise ValueError(f"candidate has no CRL for configured CA(s): {', '.join(missing)}")
    now = datetime.datetime.now(datetime.timezone.utc)
    numbers = []
    for issuer, (number, crl) in new.items():
        if issuer in old and number <= old[issuer][0]:
            raise ValueError(f"candidate CRL number {number} for {issuer} is not greater than installed number {old[issuer][0]}")
        last = parse_time(crl_field(crl, "-lastupdate"))
        nxt = parse_time(crl_field(crl, "-nextupdate"))
        if last > now + datetime.timedelta(seconds=60):
            raise ValueError(f"candidate CRL for {issuer} is not yet valid (more than 60 seconds in the future)")
        if nxt <= now:
            raise ValueError(f"candidate CRL for {issuer} has expired")
        numbers.append(f"{number}")
    old_numbers = ",".join(str(old[i][0]) for i in sorted(old)) or "none"
    fingerprint = run(["openssl", "dgst", "-sha256", "-r"], open(candidate, "rb").read()).split()[0]
    print(f"CRL_CHECK=PASS cas={len(cas)} old_numbers={old_numbers} candidate_numbers={','.join(numbers)} sha256={fingerprint}")
except (subprocess.CalledProcessError, OSError, ValueError) as exc:
    detail = exc.stderr.decode().strip() if isinstance(exc, subprocess.CalledProcessError) and exc.stderr else exc
    print(f"apply-crl: CRL validation failed: {detail}", file=sys.stderr)
    sys.exit(2)
PY

if [[ "$ACTION" == "check" ]]; then
  exit 0
fi

SERVICE="baft-tunnel@${INSTANCE}.service"
CRL_DIR="$(cd "$(dirname "$CURRENT_CRL")" && pwd -P)"
CRL_NAME="$(basename "$CURRENT_CRL")"
CURRENT_CRL="$CRL_DIR/$CRL_NAME"
[[ -d "$CRL_DIR" && -w "$CRL_DIR" ]] || die "CRL directory is not writable: $CRL_DIR"
exec 9>>"$CRL_DIR/.${CRL_NAME}.apply.lock"
flock -x 9
# Revalidate under the per-CRL lock to prevent cooperating concurrent updates.
"$0" check "$CA_CERT" "$CURRENT_CRL" "$CANDIDATE_CRL" >/dev/null
systemctl is-active --quiet "$SERVICE" || die "selected service must already be active; refusing to start or enable it"
BACKUP="$(mktemp "$CRL_DIR/.${CRL_NAME}.rollback.XXXXXX")"
STAGED="$(mktemp "$CRL_DIR/.${CRL_NAME}.staged.XXXXXX")"
INSTALLED=0
cleanup() {
  local status=$?
  rm -f "$STAGED"
  if [[ "$INSTALLED" != 2 ]]; then rm -f "$BACKUP"; fi
  exit "$status"
}
trap cleanup EXIT

install -o root -g root -m 0600 "$CURRENT_CRL" "$BACKUP"
install -o root -g root -m 0644 "$CANDIDATE_CRL" "$STAGED"
# Validate the exact staged bytes, not a candidate path that may change after preflight.
"$0" check "$CA_CERT" "$CURRENT_CRL" "$STAGED" >/dev/null
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
install -o root -g root -m 0644 "$BACKUP" "$STAGED"
mv -f "$STAGED" "$CURRENT_CRL"
INSTALLED=1
if systemctl restart "$SERVICE" && systemctl is-active --quiet "$SERVICE"; then
  rm -f "$BACKUP"
  INSTALLED=1
  echo "CRL_APPLY=ROLLED_BACK service=$SERVICE activation_status=$activation_status" >&2
  exit 1
fi
echo "CRL_APPLY=ROLLBACK_FAILED service=$SERVICE; inspect service and CRL immediately" >&2
INSTALLED=2
exit 3
