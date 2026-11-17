#!/usr/bin/env bash
# Create <docs>/rs-reviews/<YYYY-MM-DD>/<scope>.md (location from review-dir.sh; one folder per day) from the rs-review template,
# fill the automated-checks section, append a row to the day's work-log.md and refresh the README indexes.
# The agent then adds judgement findings (cards) and logs each step with review-log.sh.
#   review-report.sh [crate-dir] [--scope name] [--files "a.rs b.rs"] [--out dir] [--target triple]
# --diff <base-ref>: also run the differential review (diff-review.py) for base...HEAD and add it to the report
# --target defaults to build.target in .cargo/config.toml, or wasm32-wasip1 for a cdylib that
# references it in its Makefile (Flex/proxy-wasm policies); the release build for it is a check row.
set -uo pipefail
SK="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; BUNDLE="$(cd "$SK/../.." && pwd)"
DIR=.; SCOPE=crate; FILES=""; OUT=""; TARGET=""; DIFF_BASE=""
while [[ $# -gt 0 ]]; do case $1 in --scope) SCOPE=$2; shift 2;; --files) FILES=$2; shift 2;; --out) OUT=$2; shift 2;; --target) TARGET=$2; shift 2;; --diff) DIFF_BASE=$2; shift 2;; *) DIR=$1; shift;; esac; done
cd "$DIR" || exit 2; [[ -f Cargo.toml ]] || { echo "no Cargo.toml in $DIR"; exit 2; }
OUT=${OUT:-$(bash "$SK/scripts/review-dir.sh" .)}; mkdir -p "$OUT"
DATE="${RS_REVIEW_DATE:-$(date +%Y-%m-%d)}"; DAYDIR="$OUT/$DATE"; mkdir -p "$DAYDIR"
REPORT="$DAYDIR/$SCOPE.md"; n=2; while [[ -e "$REPORT" ]]; do REPORT="$DAYDIR/$SCOPE-$n.md"; n=$((n+1)); done
# previous report of the SAME scope: any earlier day folder, or a legacy flat file (YYYY-MM-DD-<scope>.md)
PREV=$( { ls -1 "$OUT"/*/"$SCOPE".md "$OUT"/*/"$SCOPE"-[0-9]*.md "$OUT"/*-"$SCOPE".md "$OUT"/*-"$SCOPE"-[0-9]*.md 2>/dev/null; } | grep -v "^$REPORT$" | sort | tail -1)
CRATE=$(grep -m1 '^name' Cargo.toml | cut -d'"' -f2); VER=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "not a git repo"); DIRTY=$(git status --porcelain 2>/dev/null | grep -c . || true)
[[ "${DIRTY:-0}" -gt 0 ]] && COMMIT="$COMMIT (+$DIRTY uncommitted)"
TARGETS=${FILES:-.}
TMPD=$(mktemp -d "${TMPDIR:-/tmp}/rs-review.XXXXXX"); trap 'rm -rf "$TMPD"' EXIT   # scratch files never outlive the run (rs-cleanup)
tmp=$(mktemp); section() { printf '\n### %s\n\n```text\n' "$1" >> "$tmp"; cat >> "$tmp"; printf '```\n' >> "$tmp"; }
status() { [[ $1 -eq 0 ]] && echo "PASS" || echo "FAIL"; }
bash "$BUNDLE/scripts/contract-check.sh" $TARGETS > $TMPD/rsr 2>&1; c1=$?
bash "$BUNDLE/scripts/structure-check.sh" . --strict > $TMPD/rsr2 2>&1; c2=$?
cargo fmt --all --check > $TMPD/rsr3 2>&1; c3=$?
cargo clippy --all-targets --all-features --message-format=short --quiet > $TMPD/rsr4 2>&1; c4=$?; nclip=$(grep -cE '(^|: )(warning|error)(\[|:)' $TMPD/rsr4); [[ $nclip -gt 0 && $c4 -eq 0 ]] && c4=1
cargo test --all-targets --all-features --quiet > $TMPD/rsr5 2>&1; c5=$?
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --quiet > $TMPD/rsr6 2>&1; c6=$?
bash "$BUNDLE/scripts/release-check.sh" . --no-net > $TMPD/rsr7 2>&1; c7=$?
if [[ -z "$TARGET" && -f .cargo/config.toml ]]; then TARGET=$(sed -nE 's/^[[:space:]]*target[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' .cargo/config.toml | head -1); fi
if [[ -z "$TARGET" ]] && grep -q 'cdylib' Cargo.toml && grep -qs 'wasm32-wasip1' Makefile; then TARGET=wasm32-wasip1; fi
python3 -I "$BUNDLE/scripts/port-check.py" . > $TMPD/rsr10 2>&1; c10=0; nport=$(grep -cE '^(LEAK|SQL|CFG|DEP|NOPORT)' $TMPD/rsr10); [[ $nport -gt 0 ]] && c10=1
c8=-1; if [[ -n "$TARGET" ]]; then cargo build --release --target "$TARGET" --quiet > $TMPD/rsr8 2>&1; c8=$?; fi
{ echo "| Check | Result | Detail |"; echo "|---|---|---|"
  echo "| Design Contracts | $(status $c1) | $(tail -1 $TMPD/rsr) |"
  echo "| Structure | $(status $c2) | $(tail -1 $TMPD/rsr2) |"
  echo "| rustfmt | $(status $c3) | $(grep '^Diff in' $TMPD/rsr3 | sed -E 's/^Diff in (.*) at line.*/\1/;s/:[0-9]+:?$//' | sort -u | grep -c .) file(s) differ |"
  echo "| clippy | $(status $c4) | $nclip diagnostic(s) |"
  echo "| tests | $(status $c5) | $(grep -E '^test result' $TMPD/rsr5 | awk '{p+=$4; f+=$6; i+=$8} END {printf "%d passed, %d failed, %d ignored (all suites)", p, f, i}') |"
  echo "| docs (-Dwarnings) | $(status $c6) | |"
  echo "| release gate | $(status $c7) | $(tail -1 $TMPD/rsr7) |"
  echo "| ports & adapters | $([[ $c10 -eq 0 ]] && echo PASS || echo WARN) | $nport vendor leak(s) outside adapters |"
  if [[ $c8 -ge 0 ]]; then echo "| build \`$TARGET\` (release) | $(status $c8) | $(grep -m1 -E 'error|undefined symbol' $TMPD/rsr8 | cut -c1-120) |"; fi; } > "$tmp"
grep -vE '^structure:|^design contracts:' $TMPD/rsr | head -40 | section "Design Contracts"
grep -vE '^structure:' $TMPD/rsr2 | head -40 | section "Structure"
head -60 $TMPD/rsr4 | section "clippy (first 60 lines)"
grep -E '^(PASS|WARN|FAIL|==)' $TMPD/rsr7 | section "Release gate"
[[ $c8 -gt 0 ]] && head -30 $TMPD/rsr8 | section "Target build ($TARGET)"
[[ $c10 -ne 0 ]] && head -30 $TMPD/rsr10 | section "Ports & adapters (port-check: vendor code outside adapters)"
if git rev-parse --git-dir >/dev/null 2>&1; then bash "$BUNDLE/skills/rs-git/scripts/git-check.sh" audit . 2>&1 | head -40 | section "Repository hygiene (git-check audit)"; fi
bash "$BUNDLE/skills/rs-security-review/scripts/rust-caps.sh" . 2>/dev/null | head -40 | section "Security capabilities (rust-caps: which passes apply)"
if [[ -n "$DIFF_BASE" ]]; then python3 -I "$SK/scripts/diff-review.py" --base "$DIFF_BASE" --repo . > $TMPD/rsrd 2>&1 && { printf '\n' >> "$tmp"; cat $TMPD/rsrd >> "$tmp"; printf '\n' >> "$tmp"; }; fi
python3 -I "$SK/scripts/risk-scan.py" . > $TMPD/rsr9 2>/dev/null && { printf '\n### Risk scan (candidates for the Security & contract pass)\n\n' >> "$tmp"; cat $TMPD/rsr9 >> "$tmp"; }
python3 -I - "$SK/templates/report.md" "$REPORT" "$tmp" <<PY
import os, sys
tpl,out,auto=sys.argv[1:4]; s=open(tpl).read()
prev="$PREV"
rep={"SCOPE":"$SCOPE","DATE":"$DATE","CRATE":"$CRATE","VERSION":"$VER","SCOPE_DESC":"${FILES:-whole crate}","COMMIT":"$COMMIT",
     "RS_VERSION":open("$BUNDLE/VERSION").read().strip(),"PREVIOUS":(f"[{os.path.relpath(prev, os.path.dirname(out))}]({os.path.relpath(prev, os.path.dirname(out))})" if prev else "none"),
     "VERDICT":"_pending judgement pass_","N_CRIT":"_","N_HIGH":"_","N_MED":"_","N_LOW":"_",**{f"P{i}_COUNTS":"_ | _ | _ | _ | _" for i in range(1,7)},"AUTOMATED":open(auto).read()}
for k,v in rep.items(): s=s.replace("{{"+k+"}}",v)
open(out,"w").write(s)
PY
rm -f "$tmp"
nfail=$(grep -c '| FAIL |' "$REPORT"); nwarn=$(grep -c '| WARN |' "$REPORT")
bash "$SK/scripts/review-log.sh" --out "$OUT" --day "$DATE" "Machine pass: $CRATE ($SCOPE)" "contracts, structure, fmt, clippy, tests, docs, release gate, ports, risk scan${TARGET:+, release build for $TARGET}" "$nfail failing, $nwarn warning check(s)" "$(basename "$REPORT") → Appendix: Automated checks" >/dev/null
python3 -I "$SK/scripts/review-index.py" "$OUT" >/dev/null
case $REPORT in /*) echo "report: $REPORT";; *) echo "report: $DIR/$REPORT";; esac; if [[ -n "$PREV" ]]; then echo "previous: $PREV"; fi
echo "day index: $DAYDIR/README.md · work log: $DAYDIR/work-log.md"
bash "$BUNDLE/skills/rs-cleanup/scripts/disk-guard.sh" over . >/dev/null 2>&1 || echo "disk: a budget is exceeded after this review's builds — run skills/rs-cleanup/scripts/cleanup.sh plan, then apply"
exit 0
