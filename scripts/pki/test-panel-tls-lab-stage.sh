#!/usr/bin/env bash
set -Eeuo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
STAGE=lab/real-hosts/panel-pairing-stage.sh
MAIN=lab/real-hosts/phase1-two-host-e2e.sh
for route in '/v1/pki/csr/create' '/v1/pki/csr/read' '/v1/pki/certificate/install'; do
  grep -Fq "$route" "$STAGE" || { echo "missing D2 Agent API route in two-host stage: $route" >&2; exit 1; }
done
grep -Fq -- '--network none' "$STAGE" || { echo "offline CA container must have no network" >&2; exit 1; }
grep -Fq 'service_restarted' "$STAGE" || { echo "LAB must assert certificate import does not restart the service" >&2; exit 1; }
grep -Fq 'private_key_exported' "$STAGE" || { echo "LAB must assert the private key never leaves the node" >&2; exit 1; }
grep -Fq 'pki_files' "$STAGE"
grep -Fq 'tcp-tls' "$MAIN" || { echo "two-host runner does not expose the tcp-tls panel stage" >&2; exit 1; }
grep -Fq 'LAB_PANEL_ENGINE' "$MAIN" || { echo "two-host runner does not bind the panel engine input" >&2; exit 1; }
# Drift diagnostics must preserve nftables table/set context while redacting IPv4 and IPv6 values.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cat > "$tmp/before" <<'EOF'
table inet host_guard {
 set ssh_dynamic {
  type ipv4_addr
  elements = { 198.51.100.10, 2001:db8::1 }
 }
}
EOF
cat > "$tmp/after" <<'EOF'
table inet host_guard {
 set ssh_dynamic {
  type ipv4_addr
  elements = { 198.51.100.10, 203.0.113.20, 2001:db8::1 }
 }
}
EOF
diff -U3 "$tmp/before" "$tmp/after" > "$tmp/diff" || [[ $? -eq 1 ]]
sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}(\/[0-9]+)?/<ip>/g; s/[0-9a-fA-F]{0,4}(:[0-9a-fA-F]{0,4}){2,7}(\/[0-9]+)?/<ip6>/g' "$tmp/diff" > "$tmp/redacted"
grep -Fq 'table inet host_guard' "$tmp/redacted"
grep -Fq 'set ssh_dynamic' "$tmp/redacted"
grep -Fq '<ip>' "$tmp/redacted"
grep -Fq '<ip6>' "$tmp/redacted"
! grep -Eq '198\.51\.100\.10|203\.0\.113\.20|2001:db8::1' "$tmp/redacted"
bash -n "$STAGE" "$MAIN"
echo "PANEL_TLS_LAB_STAGE_STATIC_TEST=PASS"
