#!/usr/bin/env python3
"""Risk scan for rs-review: candidates for the Security & contract pass.

  risk-scan.py <crate-dir>          # Markdown to stdout

Not a verdict — every hit is a *candidate* the reviewer must confirm or dismiss.
Scans non-test, non-generated code under src/:

  fail-open     `Err(..) => return …Continue`, `unwrap_or(false)` on matchers,
                parse-to-Null/default then use, `let _ =` on writes/sends/saves
  casts         `as u8/u16/u32/u64/usize/i32/i64` on values that may be negative/large
  log-pii       log lines printing tokens, secrets, passwords, e-mails, full claims
  reflect       replies/messages built from matched or upstream content (`preview`, `Content:`)
  properties    stream-property keys read by one crate in the workspace but written by none
  config        schema options (gcl.yaml / *schema*.json) whose property or enum value the code never mentions
  reinvention   hand-rolled standards (JSON-RPC/MCP envelopes, SSE framing, JWT parsing, OAuth, HTTP parsing,
                retry loops, base64/hex, own Pool/Cache types, vendor REST calls) when the matching SDK crate
                is not a dependency — rs-reuse-first: search the market before drafting a standard
"""
import json, os, re, subprocess, sys
from pathlib import Path

crate = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()


def src_lines(root: Path):
    """(path, lineno, line) for non-test, non-generated code."""
    for f in sorted((root / "src").rglob("*.rs")):
        if "generated" in f.parts:
            continue
        in_test = False
        for n, line in enumerate(f.read_text(errors="replace").splitlines(), 1):
            if re.match(r"\s*#\[cfg\(test\)\]", line):
                in_test = True
            if in_test:
                continue
            if line.lstrip().startswith("//"):
                continue
            yield f, n, line


CHECKS = {
    "fail-open": [
        (r"Err\([^)]*\)\s*=>\s*(return\s+)?(Flow::)?Continue", "error branch continues the request"),
        (r"is_match\([^)]*\)\s*\.unwrap_or\(false\)", "matcher error counts as no-match"),
        (r"from_(slice|str)\([^)]*\)\s*\.unwrap_or(_default)?\((Value::Null|serde_json::Value::Null)?\)", "parse failure becomes Null/default"),
        (r"let\s+_\s*=\s*[^;]*\.(set_body|save|send|write|write_all|set_header|flush|post)\(", "write/send result discarded"),
    ],
    "casts": [(r"\bas\s+(u8|u16|u32|u64|usize|i32|i64)\b", "`as` cast — use try_from if the value can be negative/large")],
    "log-pii": [
        (r"(logger|log|tracing)::(info|warn|error|debug|trace)!\(.*\b(token|secret|password|passwd|email|claims\s*=|authorization)\b", "log line may print sensitive data"),
    ],
    # only lines that build what the caller receives (not logs, not helper definitions)
    "reflect": [(r"^(?!.*logger::)(?!\s*(pub(\(\w+\))?\s+)?fn\s).*(json!|format!|with_body|set_body|\"message\").*\b(preview|matched_value|raw_body|upstream_body)\b", "reply/message may echo matched or upstream content")],
}


# Checks that span lines (rustfmt splits these): `Err(..) => { …; return …Continue… }`.
MULTI = [
    (re.compile(r"Err\([^)]*\)\s*=>\s*\{(?:(?!\n\s*\}).)*?return\s+(Flow::)?Continue", re.S), "fail-open", "error branch continues the request"),
]


def scan_multiline(root: Path, hits):
    for f in sorted((root / "src").rglob("*.rs")):
        if "generated" in f.parts:
            continue
        text = f.read_text(errors="replace").split("#[cfg(test)]")[0]
        for rx, cat, why in MULTI:
            for m in rx.finditer(text):
                n = text.count("\n", 0, m.start()) + 1
                item = f"`{f.relative_to(root)}:{n}` — {why}"
                if item not in hits[cat]:
                    hits[cat].append(item)


def scan_patterns(root: Path):
    hits = {k: [] for k in CHECKS}
    for f, n, line in src_lines(root):
        for cat, pats in CHECKS.items():
            for pat, why in pats:
                if re.search(pat, line, re.I if cat == "log-pii" else 0):
                    hits[cat].append(f"`{f.relative_to(root)}:{n}` — {why}")
                    break
    scan_multiline(root, hits)
    return hits


def workspace_crates(root: Path):
    try:
        meta = json.loads(subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
            cwd=root, capture_output=True, text=True, check=True).stdout)
        crates = [Path(p["manifest_path"]).parent for p in meta["packages"]]
    except Exception:
        crates = [root]
    if len(crates) == 1:
        # Not a workspace: sibling crates (same parent or one level up) still form the pipeline.
        base = root.parent.parent
        crates += [m.parent for m in base.glob("**/Cargo.toml")
                   if "target" not in m.parts and m.parent != root and len(m.relative_to(base).parts) <= 3]
    return crates


KEY = re.compile(r'&\[\s*"([a-z_]+)"\s*,\s*"([a-z_]+)"\s*\]')
VAR_KEY = re.compile(r'set_property\(\s*&\[\s*"([a-z_]+)"\s*,\s*[a-z_]+\s*\]')
TUPLE = re.compile(r'\(\s*"([a-z_]+)"\s*,')


WRITE_LIT = re.compile(r'set_property\(\s*&\[\s*"([a-z_]+)"\s*,\s*"([a-z_]+)"\s*\]')
READ_LIT = re.compile(r'(?:read_property|read_prop|\bprop|\beither)\((?:\s*&?\w+\s*,)?\s*&\[\s*"([a-z_]+)"\s*,\s*"([a-z_]+)"\s*\]')


def properties(crates):
    """Keys read by some crate but written by none (rustfmt-proof: whole-file regexes)."""
    written, read = set(), {}
    for c in crates:
        for f in (c / "src").rglob("*.rs") if (c / "src").exists() else []:
            text = f.read_text(errors="replace")
            written.update(f"{ns}.{k}" for ns, k in WRITE_LIT.findall(text))
            for ns in VAR_KEY.findall(text):  # keys written through a variable: take the tuple literals in the file
                written.update(f"{ns}.{k}" for k in TUPLE.findall(text))
            for ns, k in READ_LIT.findall(text):
                read.setdefault(f"{ns}.{k}", f"{c.name}/{f.relative_to(c)}")
            # also `either(&[..], &[..])` second argument
            for ns, k in re.findall(r'either\([^;]*?&\[\s*"[a-z_]+"\s*,\s*"[a-z_]+"\s*\]\s*,\s*&\[\s*"([a-z_]+)"\s*,\s*"([a-z_]+)"', text, re.S):
                read.setdefault(f"{ns}.{k}", f"{c.name}/{f.relative_to(c)}")
    # Set by the host (Envoy / Flex / proxy-wasm attributes), not by policies.
    host = ("destination.", "source.", "request.", "response.", "upstream.", "connection.", "node.", "xds.")
    return sorted((k, where) for k, where in read.items() if k not in written and not k.startswith(host))


def config_surface(root: Path):
    schemas = [p for p in [*root.glob("*schema*.json"), *root.parent.glob("*-definition/gcl.yaml"), *root.glob("gcl.yaml")]]
    if not schemas:
        return None, []
    try:
        import yaml  # type: ignore
    except ImportError:
        return schemas[0], ["(pyyaml not installed — config surface not checked)"]
    code = "\n".join(l for _, _, l in src_lines(root))
    snake = lambda k: re.sub(r"(?<!^)(?=[A-Z])", "_", k).lower()
    missing = []
    for schema in schemas:
        doc = yaml.safe_load(schema.read_text())
        props = (doc.get("spec") or doc).get("properties") or {}
        for name, spec in props.items():
            if not isinstance(spec, dict):
                continue
            if snake(name) not in code:
                missing.append(f"property `{name}` is never read")
            for value in spec.get("enum") or []:
                if isinstance(value, str) and not re.search(re.escape(value), code, re.I):
                    missing.append(f"`{name}` offers `{value}` but no code mentions it")
    return schemas[0], missing


# (regex on a source line, what it looks like, crates that normally do it, dependency names that mean "already using one")
REINVENT = [
    (r'"jsonrpc"\s*:|jsonrpc\s*:\s*(String|&str)|"2\.0"[^\n]*jsonrpc', "hand-written JSON-RPC / MCP envelope", "rmcp (official MCP SDK), jsonrpsee", {"rmcp", "jsonrpsee", "jsonrpc-core", "pdk"}),
    (r'"(data|event|id): ?"[^\n]*\\n|format!\("data: ', "hand-written SSE framing", "axum::response::sse, eventsource-stream, the MCP SDK transports", {"rmcp", "axum", "eventsource-stream", "reqwest-eventsource"}),
    (r"\.split\('\.'\)[^\n]*|split\(\"\.\"\)", "splitting a token on '.' (JWT by hand?)", "jsonwebtoken, josekit", {"jsonwebtoken", "josekit", "jwt-simple", "jsonwebtokens"}),
    (r'grant_type|client_secret|code_verifier', "OAuth parameters built by hand", "oauth2, openidconnect, rmcp `auth` feature", {"oauth2", "openidconnect", "rmcp"}),
    (r'HTTP/1\.[01]|b"\\r\\n\\r\\n"', "hand-parsed HTTP", "hyper / httparse", {"hyper", "httparse", "axum", "actix-web", "reqwest"}),
    (r'\b(fn\s+(base64|hex)_?(en|de)code|ALPHABET\s*[:=].*A-Za-z0-9)', "own base64/hex codec", "base64, hex", {"base64", "hex", "data-encoding"}),
    (r'(attempt|retries|retry_count)\b[^\n]*(\+=|<|>)', "own retry/backoff loop", "backon, tower::retry (rs-domain-error)", {"backon", "backoff", "tower", "tokio-retry", "retry"}),
    (r'struct\s+\w*(Pool|Cache)\w*', "own Pool/Cache type", "deadpool, bb8, moka", {"deadpool", "bb8", "r2d2", "moka", "lru", "mini-moka", "cached"}),
    (r'(api\.openai\.com|\.amazonaws\.com|kubernetes\.default|/api/v1/namespaces|api\.anthropic\.com)', "vendor REST API called by hand", "the vendor's SDK (aws-sdk-*, kube, async-openai …)", {"aws-config", "kube", "async-openai", "anthropic"}),
]


def dependency_names(root: Path):
    names = set()
    for m in [root / "Cargo.toml", *root.glob("*/Cargo.toml")]:
        if m.exists():
            names |= {x.replace("_", "-") for x in re.findall(r"^\s*([A-Za-z0-9_-]+)\s*(?:=|\.workspace)", m.read_text(errors="replace"), re.M)}
    return names


def reinvention(root: Path):
    deps = dependency_names(root)
    hits = []
    for f, n, line in src_lines(root):
        for rx, what, suggest, have in REINVENT:
            if deps & have:
                continue
            if re.search(rx, line):
                hits.append(f"`{f.relative_to(root)}:{n}` — {what}; check {suggest}")
                break
    return hits


def main():
    print(f"_Risk scan of `{crate.name}` — candidates for the Security & contract pass, not findings._\n")
    for cat, items in scan_patterns(crate).items():
        print(f"**{cat}** ({len(items)})")
        print("\n".join(f"- {i}" for i in items[:25]) or "- none")
        if len(items) > 25:
            print(f"- … {len(items) - 25} more")
        print()
    orphans = properties(workspace_crates(crate))
    print(f"**properties read but never written in the workspace** ({len(orphans)})")
    print("\n".join(f"- `{k}` (read in `{w}`)" for k, w in orphans) or "- none")
    print()
    rein = reinvention(crate)
    print(f"**reinvention candidates (rs-reuse-first: is there an SDK?)** ({len(rein)})")
    print("\n".join(f"- {i}" for i in rein[:25]) or "- none")
    if len(rein) > 25:
        print(f"- … {len(rein) - 25} more")
    print()
    schema, missing = config_surface(crate)
    print(f"**config surface** ({'no schema found' if schema is None else schema.name}: {len(missing)})")
    print("\n".join(f"- {m}" for m in missing) or "- none")


if __name__ == "__main__":
    main()
