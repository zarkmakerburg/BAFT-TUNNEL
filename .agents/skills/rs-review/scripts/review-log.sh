#!/usr/bin/env bash
# Append one row to the day's agent work log so developers can follow the review step by step.
#   review-log.sh [--out <rs-reviews-dir>] [--day YYYY-MM-DD] "<step>" "<what was done / looked at>" "<result>" ["<evidence: command, file, report anchor>"]
# Example:
#   review-log.sh "Judgement pass: structure" "read src/checkout/*.rs against its Design Contract" "2 findings (P3-1, P3-2)" "scripts/structure-check.sh"
# The log lives at <rs-reviews>/<day>/work-log.md and is created on first use.
set -uo pipefail
SK="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT=""; DAY="${RS_REVIEW_DATE:-$(date +%Y-%m-%d)}"
while [[ $# -gt 0 ]]; do case $1 in --out) OUT=$2; shift 2;; --day) DAY=$2; shift 2;; *) break;; esac; done
[[ $# -ge 2 ]] || { sed -n 2,8p "$0"; exit 2; }
OUT=${OUT:-$(bash "$SK/scripts/review-dir.sh" .)}
LOG="$OUT/$DAY/work-log.md"; mkdir -p "$OUT/$DAY"
if [[ ! -f "$LOG" ]]; then sed "s/{{DATE}}/$DAY/g" "$SK/templates/work-log.md" > "$LOG"; fi
esc() { printf '%s' "$1" | tr '\n' ' ' | sed 's/|/\\|/g'; }
printf '| %s | %s | %s | %s | %s |\n' "$(date +%H:%M)" "$(esc "$1")" "$(esc "$2")" "$(esc "${3:-}")" "$(esc "${4:-}")" >> "$LOG"
echo "logged: $LOG"
