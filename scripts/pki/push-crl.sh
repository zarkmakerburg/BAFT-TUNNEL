#!/usr/bin/env bash
# Operator push of an offline-signed CRL (bundle) to nodes over SSH (ADR-0017 A1).
# Nodes never fetch; the CA key never leaves the offline CA. Each node validates and
# activates the file itself with apply-crl.sh (signature, CRL number, expiry, rollback).
# Nodes are processed one at a time and the run stops at the first failure.
set -Eeuo pipefail
umask 077

usage() {
  cat <<'USAGE'
Usage:
  push-crl.sh [--dry-run] [--sudo] --crl FILE --host USER@HOST[:PORT] [--host ...] \
      --remote-ca PATH --remote-crl PATH --instance NAME [--remote-apply PATH] [--known-hosts FILE] [--key FILE]

Refuses to push a CRL bundle that crl-status.sh reports as under 72 hours or expired.
--dry-run prints the plan and contacts nothing.
USAGE
}
die() { echo "push-crl: $*" >&2; exit 2; }

DRY=0; SUDO=""; CRL=""; REMOTE_CA=""; REMOTE_CRL=""; INSTANCE=""
REMOTE_APPLY="/usr/local/lib/baft-tunnel/apply-crl.sh"; KNOWN_HOSTS=""; KEY=""
HOSTS=()
while (($#)); do
  case "$1" in
    --dry-run) DRY=1; shift ;;
    --sudo) SUDO="sudo -n "; shift ;;
    --crl) CRL="${2:?}"; shift 2 ;;
    --host) HOSTS+=("${2:?}"); shift 2 ;;
    --remote-ca) REMOTE_CA="${2:?}"; shift 2 ;;
    --remote-crl) REMOTE_CRL="${2:?}"; shift 2 ;;
    --remote-apply) REMOTE_APPLY="${2:?}"; shift 2 ;;
    --instance) INSTANCE="${2:?}"; shift 2 ;;
    --known-hosts) KNOWN_HOSTS="${2:?}"; shift 2 ;;
    --key) KEY="${2:?}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) usage; die "unknown argument: $1" ;;
  esac
done

((${#HOSTS[@]} >= 1 && ${#HOSTS[@]} <= 8)) || die "1-8 --host values are required"
[[ -n "$CRL" && -f "$CRL" && ! -L "$CRL" && -r "$CRL" ]] || die "--crl must be a readable regular non-symlink file"
[[ "$INSTANCE" =~ ^[A-Za-z0-9][A-Za-z0-9_.@-]{0,62}$ && "$INSTANCE" != *..* ]] || die "invalid --instance"
for p in "$REMOTE_CA" "$REMOTE_CRL" "$REMOTE_APPLY"; do
  [[ "$p" =~ ^/[A-Za-z0-9_./-]{1,200}$ && "$p" != *..* ]] || die "remote paths must be absolute and simple: $p"
done
for h in "${HOSTS[@]}"; do
  [[ "$h" =~ ^[A-Za-z0-9._-]{1,32}@[A-Za-z0-9.:-]{1,253}$ ]] || die "invalid --host (expected USER@HOST[:PORT])"
done

HERE="$(cd "$(dirname "$0")" && pwd -P)"
"$HERE/crl-status.sh" "$CRL" 72 || die "refusing to push a CRL that is expiring or expired (see crl-status output)"

ssh_args=(-o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=12 -o LogLevel=ERROR)
[[ -z "$KNOWN_HOSTS" ]] || ssh_args+=(-o "UserKnownHostsFile=$KNOWN_HOSTS")
[[ -z "$KEY" ]] || ssh_args+=(-i "$KEY")

done_hosts=()
for target in "${HOSTS[@]}"; do
  user="${target%%@*}"; hostport="${target#*@}"
  host="$hostport"; port=22
  if [[ "$hostport" =~ ^([^:]+):([0-9]{1,5})$ ]]; then host="${BASH_REMATCH[1]}"; port="${BASH_REMATCH[2]}"; fi
  if ((DRY)); then
    echo "PLAN node=$((${#done_hosts[@]} + 1)) stage CRL, run apply-crl.sh apply for instance $INSTANCE (no connection made)"
    done_hosts+=("$target"); continue
  fi
  stage="$(ssh "${ssh_args[@]}" -p "$port" "$user@$host" 'umask 077; mktemp -d /tmp/baft-crl-push.XXXXXX')" \
    || die "cannot stage on node $((${#done_hosts[@]} + 1)); activated before it: ${#done_hosts[@]}"
  [[ "$stage" =~ ^/tmp/baft-crl-push\.[A-Za-z0-9]+$ ]] || die "unexpected remote staging path"
  rc=0
  scp "${ssh_args[@]}" -P "$port" "$CRL" "$user@$host:$stage/candidate.crl" >/dev/null || rc=$?
  if ((rc == 0)); then
    ssh "${ssh_args[@]}" -p "$port" "$user@$host" "${SUDO}'$REMOTE_APPLY' apply '$REMOTE_CA' '$REMOTE_CRL' '$stage/candidate.crl' '$INSTANCE'" || rc=$?
  fi
  ssh "${ssh_args[@]}" -p "$port" "$user@$host" "rm -rf '$stage'" || true
  if ((rc != 0)); then
    echo "push-crl: FAILED on node $((${#done_hosts[@]} + 1)) (rolled back by apply-crl.sh on that node); activated before it: ${#done_hosts[@]}; stopping" >&2
    exit 1
  fi
  done_hosts+=("$target")
done
echo "PUSH_CRL=OK nodes=${#done_hosts[@]} dry_run=$DRY"
