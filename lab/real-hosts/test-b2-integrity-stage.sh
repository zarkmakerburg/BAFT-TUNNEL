#!/usr/bin/env bash
# Offline checks for b2-integrity-stage.sh and its dispatcher wiring; no hosts, no network.
set -Eeuo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
STAGE="$ROOT/lab/real-hosts/b2-integrity-stage.sh"
DRIVER="$ROOT/lab/real-hosts/phase1-two-host-e2e.sh"

for f in "$STAGE" "$DRIVER"; do
  bash -n "$f" || { echo "syntax error in $f" >&2; exit 1; }
done

need() { grep -Fq -- "$1" "$STAGE" || { echo "b2 stage is missing: $1" >&2; exit 1; }; }
need_in() { grep -Fq -- "$2" "$1" || { echo "$1 is missing: $2" >&2; exit 1; }; }

# The stage is a separate LAB run, sources the shared helpers instead of redefining them, and
# restores the dispatcher's EXIT trap status like the other stages do.
need '# shellcheck shell=bash'
need 'sourced by'
need 'trap '\''b2_status=$?; set +e; b2_cleanup; (exit "$b2_status"); cleanup'\'' EXIT'
for helper in ssh_host copy_to_host probe_host; do
  if grep -Eq "^(ssh_host|copy_to_host|probe_host)\(\)" "$STAGE"; then echo "stage redefines shared helper $helper" >&2; exit 1; fi
done

# Protocol 2 must be selected on both nodes and asserted in the generated configs.
grep -Fq -- '--protocol-version 2' "$STAGE" || { echo 'the stage never selects protocol generation 2' >&2; exit 1; }
test "$(grep -Fc -- 'setup init $b2_rcommon' "$STAGE")" = 2 || { echo 'both nodes must run setup init with the shared protocol-2 flags' >&2; exit 1; }
need 'protocol_version\"]==2'

# The packaging change under test: the tunnel unit carries the control socket for its instance.
need 'Environment=BAFT_TUNNEL_CONTROL_SOCKET=/run/baft-tunnel/$inst/control.sock'
need '"baft-tunnel baft-tunnel 660 socket"'
need '--register $inst --agent-listen 127.0.0.1:$B2_AGP'

# Acceptance must include the positive path and every negative the review of PR #177 relied on.
for marker in 'result"] == "pass"' 'd["bytes"] == 65536' 'control_unavailable' 'rate_limited' 'v1 forward node still relayed' 'root_refused=1' 'tunnel_user_refused=1' 'replay produced a second integrity_probe record' 'diagnostics_tier=pass' 'cleanup_clean=1'; do
  need "$marker"
done
need "c['protocol_version'] = 1"
need 'B2_RESULT protocol_v2_both=1'

# The probe is a local control-plane action: the stage must not talk to the network or print the token.
if grep -Eq '(^|[^a-z])curl ' "$STAGE"; then echo 'stage must not use curl' >&2; exit 1; fi
if grep -Eq 'echo .*\$\(cat \$AGS/token\)' "$STAGE"; then echo 'stage must not print the agent token' >&2; exit 1; fi
# Only instance-scoped runtime cleanup is allowed.
if grep -Eq 'rm -rf /run/baft-tunnel\b[^/]' "$STAGE"; then echo 'stage removes the whole runtime root' >&2; exit 1; fi
need 'rm -rf /run/baft-tunnel/$inst'

# Host drift: a before/after snapshot comparison is required, and users are only removed if we created them.
need 'b2_side_snapshot "$side" > "$LOCAL_TMP/b2-before-$side.txt"'
need 'b2_side_snapshot "$side" > "$LOCAL_TMP/b2-after-$side.txt"'
need 'B2_USERS_BEFORE'

# Dispatcher wiring: opt-in flag, exclusivity with the other stages, and the source line.
need_in "$DRIVER" 'B2="${LAB_B2:-0}"'
need_in "$DRIVER" 'LAB_B2 must be 0 or 1'
need_in "$DRIVER" 'LAB_B2 needs MVP_BINARY'
need_in "$DRIVER" 'LAB_B2 is a separate run'
need_in "$DRIVER" 'b2-integrity-stage.sh'
need_in "$DRIVER" 'b2=$B2'
if [[ "$(grep -Fc 'b2-integrity-stage.sh' "$DRIVER")" != 2 ]]; then echo 'dispatcher must reference the stage once in a shellcheck source comment and once in the source line' >&2; exit 1; fi

# The default run must stay untouched: the stage is only sourced when LAB_B2=1.
grep -A2 'if (( B2 == 1 )); then' "$DRIVER" | grep -Fq 'b2-integrity-stage.sh'

if command -v shellcheck >/dev/null 2>&1; then
  shellcheck -S warning "$STAGE" || { echo 'shellcheck reported warnings for the B2 stage' >&2; exit 1; }
fi

echo 'b2-integrity-stage tests: PASS (syntax, protocol-2 setup, probe positives and negatives, packaging wiring, cleanup scope, dispatcher opt-in)'
