#!/usr/bin/env bash
# Offline BAFT-TUNNEL node PKI helper. Keep the CA directory offline.
set -Eeuo pipefail
umask 077

die() { echo "baft-pki: $*" >&2; exit 2; }
TMP_DIR=""
# Automation and tests only: BAFT_PKI_PASSFILE names a file holding the CA passphrase so that no
# terminal prompt is needed. Operators should leave it unset and type the passphrase.
PASSIN=(); PASSOUT=()
if [[ -n "${BAFT_PKI_PASSFILE:-}" ]]; then
  [[ -f "$BAFT_PKI_PASSFILE" && -r "$BAFT_PKI_PASSFILE" ]] || die "BAFT_PKI_PASSFILE is not a readable file"
  PASSIN=(-passin "file:$BAFT_PKI_PASSFILE"); PASSOUT=(-pass "file:$BAFT_PKI_PASSFILE")
fi
cleanup() {
  if [[ -n "$TMP_DIR" && -d "$TMP_DIR" ]]; then rm -rf "$TMP_DIR"; fi
}
trap cleanup EXIT

usage() {
  cat <<'EOF'
Usage:
  baft-pki.sh init-ca CA_DIR [CA_NAME]
  baft-pki.sh new-node NODE_ID NODE_DIR
  baft-pki.sh issue-node CA_DIR NODE_ID CSR_FILE OUTPUT_DIR SERVER_NAME [VALID_DAYS]
  baft-pki.sh revoke-node CA_DIR NODE_CERT OUTPUT_DIR
  baft-pki.sh renew-crl CA_DIR OUTPUT_DIR [VALID_DAYS]
  baft-pki.sh verify-node CA_DIR CRL_FILE NODE_CERT NODE_ID SERVER_NAME

Peers refuse every connection once the CRL has expired. Run renew-crl weekly
(default validity 30 days, 2-90 allowed), check the result with crl-status.sh
and distribute it with push-crl.sh. Revocations already recorded are kept.

The CA key created by init-ca is encrypted and should remain offline.
Node keys are unencrypted EC P-256 files with mode 0600 because the
current runtime does not yet support encrypted private-key files.
EOF
}

validate_node_id() {
  [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] ||
    die "node ID must be 1-64 ASCII letters, digits, '.', '_' or '-' and start with an alphanumeric"
}

server_san_entry() {
  local value="$1"
  if python3 - "$value" <<'PYIP' >/dev/null 2>&1
import ipaddress
import sys
ipaddress.ip_address(sys.argv[1])
PYIP
  then
    printf 'IP:%s\n' "$value"
    return
  fi
  [[ ${#value} -le 253 && "$value" == *[A-Za-z]* ]] &&
    [[ "$value" =~ ^([A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?)(\.([A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?))*$ ]] ||
    die "server name must be a valid DNS name or IP literal"
  printf 'DNS:%s\n' "$value"
}

require_ca() {
  local dir="$1"
  [[ -d "$dir" && -r "$dir/ca.crt" && -r "$dir/ca.key" && -r "$dir/openssl.cnf" ]] ||
    die "CA directory must contain ca.crt, ca.key, and openssl.cnf: $dir"
}

init_ca() {
  local dir="$1" ca_name="${2:-BAFT-TUNNEL Private CA}"
  # Rollover (ADR-0017 B1): old and new CAs must have different subjects, because
  # each CRL is matched to its issuing CA by name.
  [[ "$ca_name" =~ ^[A-Za-z0-9][A-Za-z0-9\ ._-]{0,63}$ ]] || die "CA_NAME must be 1-64 characters of letters, digits, space, dot, dash or underscore"
  if [[ -e "$dir" ]] && find "$dir" -mindepth 1 -print -quit | grep -q .; then
    die "refusing to initialize a non-empty directory: $dir"
  fi
  mkdir -p "$dir/newcerts"
  chmod 700 "$dir" "$dir/newcerts"
  dir="$(cd "$dir" && pwd -P)"
  : > "$dir/index.txt"
  printf '1000\n' > "$dir/serial"
  printf '1000\n' > "$dir/crlnumber"
  cat > "$dir/openssl.cnf" <<EOF
[ ca ]
default_ca = CA_default
[ CA_default ]
database = "$dir/index.txt"
new_certs_dir = "$dir/newcerts"
certificate = "$dir/ca.crt"
private_key = "$dir/ca.key"
serial = "$dir/serial"
crlnumber = "$dir/crlnumber"
default_md = sha384
default_days = 90
default_crl_days = 30
policy = policy_node
unique_subject = no
copy_extensions = none
crl_extensions = crl_ext
[ policy_node ]
commonName = supplied
[ crl_ext ]
authorityKeyIdentifier = keyid:always
EOF
  echo "Creating encrypted offline CA key; OpenSSL will prompt for a passphrase."
  openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-384 -aes-256-cbc "${PASSOUT[@]}" -out "$dir/ca.key"
  openssl req -new -x509 "${PASSIN[@]}" -key "$dir/ca.key" -out "$dir/ca.crt" \
    -days 3650 -sha384 -subj "/CN=$ca_name" \
    -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
    -addext "keyUsage=critical,keyCertSign,cRLSign"
  openssl ca "${PASSIN[@]}" -gencrl -batch -config "$dir/openssl.cnf" -out "$dir/ca.crl"
  chmod 600 "$dir/ca.key" "$dir/index.txt" "$dir/serial" "$dir/crlnumber"
  chmod 644 "$dir/ca.crt" "$dir/ca.crl" "$dir/openssl.cnf"
  echo "CA initialized: $dir"
  openssl x509 -in "$dir/ca.crt" -noout -fingerprint -sha256
}

new_node() {
  local node_id="$1" node_dir="$2"
  validate_node_id "$node_id"
  mkdir -p "$node_dir"
  chmod 700 "$node_dir"
  [[ ! -e "$node_dir/node.key" && ! -e "$node_dir/node.csr" ]] ||
    die "node key or CSR already exists; refusing to overwrite"
  # SEC1 PEM is directly supported by rustls-ring. Older OpenSSL PKCS#8 output can omit
  # the public-key field required by ring; runtime also normalizes legacy PKCS#8 in memory.
  openssl ecparam -name prime256v1 -genkey -noout -out "$node_dir/node.key"
  chmod 600 "$node_dir/node.key"
  openssl req -new -key "$node_dir/node.key" -out "$node_dir/node.csr" -subj "/CN=$node_id"
  openssl req -in "$node_dir/node.csr" -verify -noout >/dev/null
  local key_info
  key_info="$(openssl pkey -in "$node_dir/node.key" -text -noout)"
  [[ "$key_info" == *"ASN1 OID: prime256v1"* ]] || die "generated node key is not EC P-256"
  chmod 600 "$node_dir/node.csr"
  echo "Created node key and CSR in $node_dir. Keep node.key on this node; transfer only node.csr to the offline CA."
}

issue_node() {
  local ca_dir="$1" node_id="$2" csr="$3" out_dir="$4" server_dns="$5" days="${6:-90}"
  validate_node_id "$node_id"
  local server_san
  server_san="$(server_san_entry "$server_dns")"
  require_ca "$ca_dir"
  [[ -r "$csr" ]] || die "CSR is not readable: $csr"
  [[ "$days" =~ ^[0-9]+$ ]] && (( days >= 1 && days <= 397 )) ||
    die "certificate validity must be 1-397 days"
  local subject key_info
  subject="$(openssl req -in "$csr" -noout -subject -nameopt RFC2253 | sed 's/^subject=//')"
  [[ "$subject" == "CN=$node_id" ]] || die "CSR subject CN does not exactly match requested node ID"
  openssl req -in "$csr" -verify -noout >/dev/null
  key_info="$(openssl req -in "$csr" -pubkey -noout | openssl pkey -pubin -text -noout)"
  [[ "$key_info" == *"ASN1 OID: prime256v1"* ]] || die "CSR public key must be EC P-256"
  mkdir -p "$out_dir"
  chmod 700 "$out_dir"
  [[ ! -e "$out_dir/node.crt" && ! -e "$out_dir/ca.crt" && ! -e "$out_dir/ca.crl" ]] ||
    die "node.crt, ca.crt, or ca.crl already exists in output directory"
  TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/baft-pki.XXXXXX")"
  cat > "$TMP_DIR/extensions.cnf" <<EOF
[node_ext]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=URI:urn:baft-tunnel:node:$node_id,$server_san
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer
EOF
  openssl ca "${PASSIN[@]}" -batch -notext -config "$ca_dir/openssl.cnf" \
    -extfile "$TMP_DIR/extensions.cnf" -extensions node_ext -days "$days" \
    -in "$csr" -out "$out_dir/node.crt"
  openssl ca "${PASSIN[@]}" -gencrl -batch -config "$ca_dir/openssl.cnf" -out "$TMP_DIR/ca.crl"
  cp "$ca_dir/ca.crt" "$out_dir/ca.crt"
  cp "$TMP_DIR/ca.crl" "$out_dir/ca.crl"
  chmod 644 "$out_dir/node.crt" "$out_dir/ca.crt" "$out_dir/ca.crl"
  echo "Issued certificate for node $node_id. The CA private key was not copied."
  openssl x509 -in "$out_dir/node.crt" -noout -fingerprint -sha256
  rm -rf "$TMP_DIR"
  TMP_DIR=""
}

revoke_node() {
  local ca_dir="$1" cert="$2" out_dir="$3"
  require_ca "$ca_dir"
  [[ -r "$cert" ]] || die "certificate is not readable: $cert"
  openssl verify -no_check_time -CAfile "$ca_dir/ca.crt" "$cert" >/dev/null ||
    die "certificate is not issued by this CA"
  local san
  san="$(openssl x509 -in "$cert" -noout -ext subjectAltName)"
  [[ "$san" == *"URI:urn:baft-tunnel:node:"* ]] || die "certificate has no BAFT-TUNNEL node URI SAN"
  mkdir -p "$out_dir"
  chmod 700 "$out_dir"
  TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/baft-pki.XXXXXX")"
  openssl ca "${PASSIN[@]}" -batch -config "$ca_dir/openssl.cnf" -revoke "$cert"
  openssl ca "${PASSIN[@]}" -gencrl -batch -config "$ca_dir/openssl.cnf" -out "$TMP_DIR/ca.crl"
  cp "$ca_dir/ca.crt" "$out_dir/ca.crt"
  cp "$TMP_DIR/ca.crl" "$out_dir/ca.crl"
  chmod 644 "$out_dir/ca.crt" "$out_dir/ca.crl"
  echo "Certificate revoked. Distribute the updated ca.crl to both peers and restart both endpoint processes."
  rm -rf "$TMP_DIR"
  TMP_DIR=""
}

renew_crl() {
  local ca_dir="$1" out_dir="$2" days="${3:-30}"
  require_ca "$ca_dir"
  [[ "$days" =~ ^[0-9]{1,2}$ ]] && (( days >= 2 && days <= 90 )) ||
    die "CRL validity must be 2-90 days (push-crl.sh refuses CRLs with under 72 hours left)"
  mkdir -p "$out_dir"
  chmod 700 "$out_dir"
  [[ ! -e "$out_dir/ca.crl" ]] || die "ca.crl already exists in output directory; use an empty directory"
  TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/baft-pki.XXXXXX")"
  # -crldays overrides default_crl_days, so CAs created before this command (7 days) also renew
  # for the requested period. The CA database is reused: every earlier revocation stays listed.
  openssl ca "${PASSIN[@]}" -gencrl -batch -crldays "$days" -config "$ca_dir/openssl.cnf" -out "$TMP_DIR/ca.crl"
  openssl crl -in "$TMP_DIR/ca.crl" -noout -CAfile "$ca_dir/ca.crt" >/dev/null 2>&1 ||
    die "renewed CRL does not verify against ca.crt"
  [[ -e "$out_dir/ca.crt" ]] || cp "$ca_dir/ca.crt" "$out_dir/ca.crt"
  cp "$TMP_DIR/ca.crl" "$out_dir/ca.crl"
  chmod 644 "$out_dir/ca.crt" "$out_dir/ca.crl"
  echo "CRL renewed for $days days. Check it with crl-status.sh, then distribute it with push-crl.sh."
  openssl crl -in "$out_dir/ca.crl" -noout -crlnumber -nextupdate
  rm -rf "$TMP_DIR"
  TMP_DIR=""
}

verify_node() {
  local ca_dir="$1" crl="$2" cert="$3" node_id="$4" server_dns="$5"
  validate_node_id "$node_id"
  local expected_server_san
  expected_server_san="$(server_san_entry "$server_dns")"
  require_ca "$ca_dir"
  [[ -r "$crl" && -r "$cert" ]] || die "CRL or certificate is not readable"
  openssl verify -purpose any -CAfile "$ca_dir/ca.crt" -CRLfile "$crl" -crl_check "$cert"
  local san uri_sans server_sans
  san="$(openssl x509 -in "$cert" -noout -ext subjectAltName | tr ',' '\n')"
  uri_sans="$(printf '%s\n' "$san" | sed -n 's/.*URI:\([^[:space:]]*\).*/\1/p')"
  server_sans="$(printf '%s\n' "$san" | sed -n -e 's/.*DNS:\([^[:space:]]*\).*/DNS:\1/p' -e 's/.*IP Address:\([^[:space:]]*\).*/IP:\1/p')"
  [[ "$uri_sans" == "urn:baft-tunnel:node:$node_id" ]] ||
    die "certificate must contain exactly one BAFT-TUNNEL node URI SAN matching $node_id"
  [[ "$server_sans" == "$expected_server_san" ]] ||
    die "certificate must contain exactly one server SAN matching $expected_server_san"
  openssl x509 -in "$cert" -noout -fingerprint -sha256
}

[[ $# -ge 1 ]] || { usage; exit 2; }
command="$1"
shift
case "$command" in
  init-ca) [[ $# == 1 || $# == 2 ]] || { usage; exit 2; }; init_ca "$@" ;;
  new-node) [[ $# == 2 ]] || { usage; exit 2; }; new_node "$1" "$2" ;;
  issue-node) [[ $# == 5 || $# == 6 ]] || { usage; exit 2; }; issue_node "$@" ;;
  revoke-node) [[ $# == 3 ]] || { usage; exit 2; }; revoke_node "$@" ;;
  renew-crl) [[ $# == 2 || $# == 3 ]] || { usage; exit 2; }; renew_crl "$@" ;;
  verify-node) [[ $# == 5 ]] || { usage; exit 2; }; verify_node "$@" ;;
  -h|--help|help) usage ;;
  *) usage; die "unknown command: $command" ;;
esac
