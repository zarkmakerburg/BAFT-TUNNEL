#!/usr/bin/env bash
# rust-caps — which security passes apply to this crate? Cheap, conservative capability probe.
#   rust-caps.sh [dir] [--json]
# A false "true" only costs one extra pass; a false "false" would skip a whole class — so patterns are wide.
# Output: one `has_<cap>=true|false` per line (or JSON), then the passes each capability switches on.
set -uo pipefail
DIR="."; JSON=0
for a in "$@"; do case "$a" in --json) JSON=1;; *) DIR="$a";; esac; done
cd "$DIR" 2>/dev/null || { echo "no such dir: $DIR" >&2; exit 2; }
find . -name '*.rs' -not -path '*/target/*' -print -quit | grep -q . || { echo "no .rs files under $DIR" >&2; exit 2; }
probe() { grep -rlE --include='*.rs' --exclude-dir=target --exclude-dir=.git "$1" . 2>/dev/null | head -1 | grep -q .; }
probe_toml() { grep -rlE --include='Cargo.toml' --exclude-dir=target "$1" . 2>/dev/null | head -1 | grep -q .; }
declare_cap() { if "$2" "$3"; then echo "$1=true"; else echo "$1=false"; fi; }
out=$(
  declare_cap has_unsafe        probe '\bunsafe[[:space:]]+(extern|fn|impl|trait)\b|\bunsafe[[:space:]]*\{'
  declare_cap has_ffi           probe 'extern[[:space:]]+"(C|system|C-unwind)"|extern[[:space:]]*\{|\b(CString|CStr)\b|\blibc::|bindgen|cbindgen|\bc_void\b|#\[no_mangle\]'
  declare_cap has_concurrency   probe 'std::(thread|sync)|parking_lot::|crossbeam|rayon::|tokio::sync|Atomic[A-Za-z0-9]*|UnsafeCell|static[[:space:]]+mut|unsafe[[:space:]]+impl[[:space:]]+(Send|Sync)|memmap2::|sigaction|signal_hook'
  declare_cap has_async         probe '\basync[[:space:]]+(fn|move|\{)|\.await\b|tokio::|async_std::|futures::'
  declare_cap has_packed_repr   probe '#!?\[repr\([^]]*packed'
  declare_cap has_fs_io         probe '\bPathBuf\b|\bPath\b|\bfs::|File::(open|create)|OpenOptions|read_dir'
  declare_cap has_untrusted_deser probe 'serde_json::from_|serde_yaml::from_|toml::from_str|ron::|ciborium|bincode::|postcard::|prost|rmp_serde|quick_xml|Json<|from_slice|from_reader'
  declare_cap has_network       probe 'axum::|actix_web|warp::|hyper::|tonic::|TcpListener|UdpSocket|reqwest::|tungstenite|rmcp::'
  declare_cap has_drop_cleanup  probe 'impl([[:space:]]*<[^>]*>)?[[:space:]]+Drop[[:space:]]+for|process::exit|mem::forget|ManuallyDrop'
  declare_cap has_serialization_impl probe 'impl([[:space:]]*<[^>]*>)?[[:space:]]+(Serialize|Deserialize)(<[^>]*>)?[[:space:]]+for|serialize_struct|#\[serde\((with|serialize_with|deserialize_with)'
  declare_cap has_secrets       probe 'password|passwd|secret|api[_-]?key|private[_-]?key|\btoken\b|Zeroize|secrecy::'
  if probe_toml '^[[:space:]]*panic[[:space:]]*=[[:space:]]*"abort"'; then echo "panic_abort=true"; else echo "panic_abort=false"; fi
  if [ -n "$(find . -name build.rs -not -path '*/target/*' -print -quit)" ]; then echo "has_build_rs=true"; else echo "has_build_rs=false"; fi
  if probe_toml 'proc-macro[[:space:]]*=[[:space:]]*true'; then echo "has_proc_macro=true"; else echo "has_proc_macro=false"; fi
)
if (( JSON )); then
  printf '{'; first=1; while IFS='=' read -r k v; do (( first )) || printf ','; first=0; printf '"%s":%s' "$k" "$v"; done <<< "$out"; printf '}\n'; exit 0
fi
echo "$out"
echo
echo "passes switched on (rs-security-review):"
get() { echo "$out" | grep -q "^$1=true"; }
echo "  always: panic-on-input · resource-exhaustion · error-handling (discarded results, drop) · logic (Ord/Eq/Hash, key mutation) · hygiene (lints, MSRV, supply chain)"
get has_unsafe && echo "  + unsafe/memory: rs-unsafe checklists (review-unsafe, common-pitfalls) — every memory-safety class needs unsafe"
get has_ffi && echo "  + ffi: panic across extern \"C\", CString lifetime, ABI/repr(C) — rs-unsafe ffi-* rules"
get has_concurrency && echo "  + concurrency: sec-lock-order-and-reentrancy, sec-atomic-check-then-act, Send/Sync (rs-unsafe safety-05)"
get has_async && echo "  + async: blocking in async, cancel safety, select! bias — rs-concurrency async-*"
get has_fs_io && echo "  + filesystem: sec-path-traversal, sec-toctou-filesystem"
get has_untrusted_deser && echo "  + deserialization: sec-recursion-depth-dos, sec-resource-exhaustion, sec-serialization-integrity"
get has_network && echo "  + network edge: threat model REMOTE by default; auth (rs-domain-auth), policy (rs-domain-policy), sec-pointer-exposure"
get has_drop_cleanup && echo "  + drop/exit: sec-drop-panic-and-skip"
get has_secrets && echo "  + secrets: sec-zeroize-secrets, sec-constant-time-compare, obs-no-sensitive-data"
get has_packed_repr && echo "  + layout: unaligned refs to packed fields — rs-unsafe"
get has_build_rs && echo "  + build-time code: build.rs runs on every build — sec-supply-chain-build-time-code"
get has_proc_macro && echo "  + proc macros run on the build machine — sec-supply-chain-build-time-code"
exit 0
