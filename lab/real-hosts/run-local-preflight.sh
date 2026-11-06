#!/usr/bin/env bash
set -Eeuo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
config="$root/lab/real-hosts/.env"
[[ -f "$config" ]] || { echo 'Copy .env.example to lab/real-hosts/.env and fill the six test hosts and credential paths.' >&2; exit 2; }
# shellcheck disable=SC1090
set -a
. "$config"
set +a
exec bash "$root/lab/real-hosts/preflight-matrix.sh"
