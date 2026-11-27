#!/usr/bin/env bash
# Shared runtime-directory checks for the two-host LAB scripts.
#
# /run/baft-tunnel is owned by the product, not by a LAB run. It has exactly two children:
#   control  the request rendezvous (root:baft-tunnel-agent 0770)
#   manage   the control-manage socket directory (root:root 0755, `*.manage.sock`)
# Both are declared by the shipped tmpfiles.d entry and the control-manage socket template, so a
# cleanup that treats `manage` as an unknown resource blocks every run that started the manage
# socket unit. These helpers accept those two and refuse everything else; they never delete.

# A known manage directory: a plain directory owned by `expect_owner` (root:root in production)
# whose entries are sockets with that owner and a `*.manage.sock` name (the control-manage units
# recreate them). Anything else is unknown. The owner is a parameter so a non-root regression test
# can still exercise the type and name rules without weakening the production default.
baft_manage_dir_known() {
  local dir="$1"
  local expect_owner="${2:-root:root}"
  local entry
  [[ -d "$dir" && ! -L "$dir" ]] || {
    echo "manage runtime path is not a plain directory: $dir" >&2
    return 1
  }
  [[ "$(stat -c '%U:%G' "$dir")" == "$expect_owner" ]] || {
    echo "manage runtime directory ownership changed: $dir" >&2
    return 1
  }
  while IFS= read -r -d '' entry; do
    [[ -S "$entry" && ! -L "$entry" ]] || {
      echo "unexpected manage runtime entry: $entry" >&2
      return 1
    }
    [[ "$(basename -- "$entry")" == *.manage.sock ]] || {
      echo "unexpected manage socket name: $entry" >&2
      return 1
    }
    [[ "$(stat -c '%U' "$entry")" == "${expect_owner%%:*}" ]] || {
      echo "manage socket is not root-owned: $entry" >&2
      return 1
    }
  done < <(find "$dir" -mindepth 1 -maxdepth 1 -print0)
  return 0
}

# Every child of the runtime parent must be one of the allowed paths. Unknown children are named and
# preserved; the caller must stop rather than remove them.
baft_runtime_children_known() {
  local parent="$1"
  shift
  local child allowed known
  [[ -d "$parent" && ! -L "$parent" ]] || {
    echo "runtime parent is missing or unsafe: $parent" >&2
    return 1
  }
  while IFS= read -r -d '' child; do
    known=0
    for allowed in "$@"; do
      [[ "$child" == "$allowed" ]] && known=1
    done
    if ((known == 0)); then
      echo "unexpected runtime resource $child; preserving it" >&2
      return 1
    fi
  done < <(find "$parent" -mindepth 1 -maxdepth 1 -print0)
  return 0
}
