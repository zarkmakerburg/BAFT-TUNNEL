#!/usr/bin/env bash
# Report-only CRL freshness check (ADR-0017 A1). Never fetches or changes anything.
# Usage: crl-status.sh CRL_BUNDLE [WARN_HOURS]   (default 72)
# Prints one line per CRL: issuer, CRL number, nextUpdate, hours remaining.
# Exit 0 = all CRLs fresh, 1 = at least one under WARN_HOURS or expired, 2 = usage/parse error.
set -Eeuo pipefail
die() { echo "crl-status: $*" >&2; exit 2; }
[[ $# == 1 || $# == 2 ]] || die "usage: crl-status.sh CRL_BUNDLE [WARN_HOURS]"
FILE="$1"; WARN="${2:-72}"
[[ -f "$FILE" && ! -L "$FILE" && -r "$FILE" ]] || die "expected readable regular non-symlink file: $FILE"
[[ "$WARN" =~ ^[0-9]{1,5}$ ]] || die "WARN_HOURS must be a number"
command -v openssl >/dev/null 2>&1 || die "openssl is required"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/baft-crl-status.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
awk -v dir="$TMP" '/-----BEGIN X509 CRL-----/{n++; f=sprintf("%s/crl-%03d.pem", dir, n)} f{print > f} /-----END X509 CRL-----/{close(f); f=""}' "$FILE"
shopt -s nullglob
CRLS=("$TMP"/crl-*.pem)
((${#CRLS[@]} > 0)) || die "no PEM CRLs found"
((${#CRLS[@]} <= 8)) || die "too many CRLs in bundle"
now="$(date -u +%s)"; status=0
for crl in "${CRLS[@]}"; do
  issuer="$(openssl crl -in "$crl" -noout -issuer -nameopt RFC2253 | sed 's/^issuer=//')" || die "unparseable CRL"
  number="$(openssl crl -in "$crl" -noout -crlnumber 2>/dev/null | sed 's/^crlNumber=//' || true)"
  next="$(openssl crl -in "$crl" -noout -nextupdate | sed 's/^nextUpdate=//')"
  [[ -n "$next" ]] || die "CRL has no nextUpdate"
  next_epoch="$(date -u -d "$next" +%s)" || die "cannot parse nextUpdate"
  hours=$(( (next_epoch - now) / 3600 ))
  state=OK
  if (( next_epoch <= now )); then state=EXPIRED; status=1
  elif (( hours < WARN )); then state=WARN; status=1; fi
  echo "$state issuer=$issuer crl_number=${number:-unknown} next_update=\"$next\" hours_remaining=$hours"
done
exit "$status"
