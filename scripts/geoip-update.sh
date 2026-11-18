#!/usr/bin/env bash
# Install or refresh the offline country database (DB-IP "IP to Country Lite", CC BY 4.0,
# https://db-ip.com). Operator-run; the agent itself never downloads anything.
# Usage: sudo scripts/geoip-update.sh [STATE_DIR] [YYYY-MM]
set -euo pipefail
dir="${1:-/var/lib/baft-tunnel}"
ym="${2:-$(date -u +%Y-%m)}"
[[ "$ym" =~ ^[0-9]{4}-[0-9]{2}$ ]] || { echo "bad month" >&2; exit 2; }
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
curl -fsSL --proto '=https' --max-filesize 33554432 -o "$tmp/db.gz" \
  "https://download.db-ip.com/free/dbip-country-lite-$ym.csv.gz"
gzip -dc "$tmp/db.gz" | head -c 67108864 > "$tmp/db.csv"
lines="$(wc -l < "$tmp/db.csv")"
(( lines > 1000 )) || { echo "downloaded file looks wrong ($lines lines)" >&2; exit 1; }
grep -Eq '^"?[0-9.]+"?,"?[0-9.]+"?,"?[A-Z]{2}"?$' "$tmp/db.csv" || { echo "unexpected format" >&2; exit 1; }
install -d -m 0755 "$dir"
install -m 0644 "$tmp/db.csv" "$dir/geoip-country.csv.new"
mv "$dir/geoip-country.csv.new" "$dir/geoip-country.csv"
echo "installed $lines ranges into $dir/geoip-country.csv"
echo "IP geolocation by DB-IP.com (CC BY 4.0)"
