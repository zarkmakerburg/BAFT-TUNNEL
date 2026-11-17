#!/usr/bin/env python3
"""Differential review triage for a change set (branch, PR, MR, commit range) — any Git host.

  diff-review.py [--base <ref>] [--head <ref>] [--repo <dir>] [--out <file.md>|-]

Answers, from git alone: what changed, where is the risk, how far does it reach, is it tested, did it remove a
check that an earlier fix added — and in what order should a human read it.  Output is a Markdown section that
rs-review puts in the report (and a reading order for the walkthrough).  It nominates; the reviewer decides.

Method (rs-skills original): triage by risk, history of removed code, blast radius, test gaps.

  1 triage        classify each changed file HIGH / MEDIUM / LOW from its path and from what the hunks touch
  2 removed code  removed lines that look like validation / checks / denials, and whether `git log -S` shows a
                  fix/security commit introduced them (removal of a fix = red flag)
  3 added hazards added unwrap/expect/as-casts/unsafe/Command/fail-open shapes in non-test code
  4 blast radius  callers of each changed fn across the repo (git grep), ≥ 20 = wide
  5 test gaps     changed fns not mentioned by any test file, and src files changed without any test change
  6 reading order definitions → implementations → wiring → tests, grouped by feature module
"""
import os
import re
import subprocess
import sys
from collections import defaultdict


def git(repo, *args, check=True):
    r = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True)
    if check and r.returncode != 0:
        sys.exit(f"git {' '.join(args)} failed: {r.stderr.strip()}")
    return r.stdout


def default_base(repo):
    ref = git(repo, "symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD", check=False).strip()
    if ref:
        return ref
    for cand in ("origin/main", "origin/master", "main", "master"):
        if git(repo, "rev-parse", "-q", "--verify", cand, check=False).strip():
            return cand
    return "HEAD~1"


HIGH_PATH = re.compile(r"(auth|jwt|oauth|oidc|token|session|polic|authz|crypto|secret|password|credential|tls|sandbox|permission|acl|rbac|signature|verif|decrypt|encrypt|jwks|masking|redact|inspect)", re.I)
HIGH_CODE = re.compile(r"\bunsafe\b|\bextern\s+\"|#\[no_mangle\]|\btransmute\b|\bset_len\b|\bfrom_raw|\bCommand::new\b|std::process::|\bfs::(remove|rename|write)|\bcatch_unwind\b|serde_json::from_|from_slice|\bbuild\.rs\b")
CHECK_REMOVED = re.compile(r"\b(ensure!|assert!|verify\w*|validate\w*|check\w*|deny\w*|reject\w*|forbid\w*|allow_?list|canonicalize|constant_time|bail!)\b|return\s+Err\(|\.is_empty\(\)|\.starts_with\(|\blen\(\)\s*[<>=]")
SECURITY_SUBJECT = re.compile(r"(fix|security|cve|vuln|auth|bypass|overflow|panic|injection|traversal|dos|leak|sanitiz|harden)", re.I)
HAZARD_ADDED = [
    (re.compile(r"\.unwrap\(\)|\.expect\("), "unwrap/expect added (panic on input?)"),
    (re.compile(r"\bas\s+(u8|u16|u32|u64|usize|i8|i16|i32|i64|isize)\b"), "`as` integer cast added (truncation/sign)"),
    (re.compile(r"\bunsafe\b"), "unsafe added"),
    (re.compile(r"Command::new|std::process::Command"), "process spawn added"),
    (re.compile(r"unwrap_or\((true|Flow::Continue)\)|Err\([^)]*\)\s*=>\s*(return\s+)?(Flow::)?Continue"), "fail-open shape added"),
    (re.compile(r"let\s+_\s*=\s*[^;]*\.(write|write_all|send|flush|save|set_body)\("), "write/send result discarded"),
    (re.compile(r"with_capacity\(|vec!\[[^\]]*;\s*\w+\]"), "allocation sized from a variable (check it is bounded)"),
    (re.compile(r"\.join\((?!\s*\")"), "Path::join with a non-literal (absolute/`..` components replace or escape the base)"),
]
SRC = re.compile(r"\.rs$")
TESTISH = re.compile(r"(^|/)(tests?|benches|examples)/|_test\.rs$|/tests\.rs$")


def is_test_path(p):
    return bool(TESTISH.search(p))


def main(argv):
    repo, base, head, out = ".", None, "HEAD", "-"
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--base": base = argv[i + 1]; i += 2
        elif a == "--head": head = argv[i + 1]; i += 2
        elif a == "--repo": repo = argv[i + 1]; i += 2
        elif a == "--out": out = argv[i + 1]; i += 2
        elif a in ("-h", "--help"): print(__doc__); return 0
        else: sys.exit(f"unknown argument {a}")
    repo = os.path.abspath(repo)
    top = git(repo, "rev-parse", "--show-toplevel").strip()
    base = base or default_base(top)
    mb = git(top, "merge-base", base, head, check=False).strip() or base
    rng = f"{mb}..{head}"
    stat = git(top, "diff", "--numstat", "-M", rng)
    files = []
    for line in stat.splitlines():
        a, d, p = line.split("\t", 2)
        if " => " in p or "{" in p:   # rename: keep the new path
            p = re.sub(r"\{[^}]*=> ([^}]*)\}", r"\1", p).replace("//", "/")
            p = p.split(" => ")[-1]
        files.append((p, 0 if a == "-" else int(a), 0 if d == "-" else int(d)))
    commits = git(top, "log", "--oneline", rng).splitlines()
    patch = git(top, "diff", "-U0", "--no-color", "--no-ext-diff", rng)

    # split patch per file
    per_file = defaultdict(lambda: {"added": [], "removed": [], "hunks": []})
    cur = None
    for line in patch.splitlines():
        m = re.match(r"^\+\+\+ b/(.*)$", line)
        if m: cur = m.group(1); continue
        if line.startswith("--- ") or line.startswith("diff --git") or line.startswith("index "): continue
        if cur is None: continue
        if line.startswith("@@"):
            per_file[cur]["hunks"].append(line); continue
        if line.startswith("+") and not line.startswith("+++"): per_file[cur]["added"].append(line[1:])
        elif line.startswith("-") and not line.startswith("---"): per_file[cur]["removed"].append(line[1:])

    ALL_ADDED = {l.strip() for v in per_file.values() for l in v["added"]}
    rows, red_flags, hazards = [], [], defaultdict(list)
    changed_fns = {}
    for p, a, d in files:
        info = per_file.get(p, {"added": [], "removed": [], "hunks": []})
        reasons = []
        risk = "LOW"
        if is_test_path(p) or p.endswith((".md", ".txt")):
            risk = "LOW"
        else:
            risk = "MEDIUM" if (SRC.search(p) or p.endswith(("Cargo.toml", "build.rs"))) else "LOW"
            if HIGH_PATH.search("/".join(p.split("/")[-2:])): risk, _ = "HIGH", reasons.append("sensitive file/module name")   # last two components only: the repo's own directory names say nothing
            body = "\n".join(info["added"] + info["removed"])
            if SRC.search(p) and HIGH_CODE.search(body): risk, _ = "HIGH", reasons.append("touches unsafe/FFI/process/deserialisation")
            if p.endswith("Cargo.toml") and re.search(r"^[+-]\s*[\w-]+\s*=", body, re.M): reasons.append("dependency change") ; risk = "HIGH" if risk != "HIGH" else risk
            if p.endswith("build.rs"): risk = "HIGH"; reasons.append("build script (runs at build time)")
        all_added = ALL_ADDED
        # a check that reappears elsewhere in the diff was moved, not removed
        removed_checks = [l.strip() for l in info["removed"] if CHECK_REMOVED.search(l) and l.strip() and not l.strip().startswith("//") and l.strip() not in all_added]
        added_checks = [l for l in info["added"] if CHECK_REMOVED.search(l)]
        if removed_checks and len(removed_checks) > len(added_checks) and not is_test_path(p):
            risk = "HIGH"; reasons.append(f"{len(removed_checks)} check-like line(s) removed vs {len(added_checks)} added")
            for snippet in removed_checks[:6]:
                red_flags.append((p, snippet))
        if not is_test_path(p) and SRC.search(p):
            for l in info["added"]:
                if l.strip().startswith("//"): continue
                for rx, what in HAZARD_ADDED:
                    if rx.search(l): hazards[what].append((p, l.strip()[:110])); break
            for h in info["hunks"]:
                m = re.search(r"@@.*@@\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(\w+)", h)
                if m: changed_fns.setdefault(m.group(1), p)
            for l in info["added"]:
                m = re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(\w+)", l)
                if m: changed_fns.setdefault(m.group(1), p)
        rows.append((p, a, d, risk, "; ".join(reasons)))

    # history of removed check-like code: was it introduced by a fix?
    history = []
    seen = set()
    for p, snippet in red_flags[:20]:
        key = snippet[:80]
        if key in seen or len(snippet) < 12: continue
        seen.add(key)
        log = git(top, "log", "-S" + snippet[:120], "--oneline", "-n", "3", mb, "--", p, check=False).strip().splitlines()
        sec = [l for l in log if SECURITY_SUBJECT.search(l)]
        if sec: history.append((p, snippet, sec[0]))

    # blast radius
    blast = []
    for name, p in list(changed_fns.items())[:40]:
        if len(name) < 4 or name in ("new", "from", "default", "main", "test", "drop", "fmt", "eq", "hash", "clone"): continue
        out_ = git(top, "grep", "-c", "-w", name, head, "--", "*.rs", check=False)
        total = sum(int(x.rsplit(":", 1)[1]) for x in out_.splitlines() if ":" in x) - 1  # minus the definition
        if total >= 3: blast.append((name, p, max(total, 0)))
    blast.sort(key=lambda x: -x[2])

    # test gaps
    test_text = git(top, "grep", "-l", "-E", r"#\[(cfg\(test\)|test|tokio::test)\]", head, "--", "*.rs", check=False).splitlines()
    test_files = [t.split(":", 1)[-1] for t in test_text]
    test_corpus = ""
    for t in test_files[:200]:
        test_corpus += git(top, "show", f"{head}:{t}", check=False)
    gaps = [(n, p) for n, p in changed_fns.items() if len(n) >= 4 and not re.search(r"\b" + re.escape(n) + r"\b", test_corpus)]
    src_changed = [p for p, *_ in rows if SRC.search(p) and not is_test_path(p)]
    tests_changed = [p for p, *_ in rows if is_test_path(p)]
    # unit tests live in-file (`#[cfg(test)] mod tests`): a diff that adds #[test] fns counts as a test change
    if any(re.search(r"#\[(test|tokio::test|cfg\(test\))\]", l) for v in per_file.values() for l in v["added"]):
        tests_changed.append("(in-file tests added)")

    high = sum(1 for r in rows if r[3] == "HIGH")
    n = len(rows)
    depth = "DEEP (read every changed file and its callees)" if n < 20 else "FOCUSED (HIGH files fully, MEDIUM one hop out)" if n < 200 else "SURGICAL (HIGH paths only; say what was not read)"

    # reading order: group by feature module, within group: model/ports -> impl -> wiring -> tests
    def group(p):
        parts = p.split("/")
        if "src" in parts:
            k = parts.index("src")
            return "/".join(parts[:k + 2]) if len(parts) > k + 2 else "/".join(parts[:k + 1])
        return parts[0] if len(parts) > 1 else "(root)"

    def stage(p):
        base_ = os.path.basename(p)
        if is_test_path(p): return 4
        if re.search(r"(model|types|ports?|error|trait|schema)", base_): return 0
        if base_ in ("main.rs", "lib.rs", "mod.rs", "wiring.rs", "app.rs") or base_.endswith(".toml"): return 3
        return 1 if "adapter" not in p else 2
    order = defaultdict(list)
    for p, a, d, risk, why in rows:
        order[group(p)].append((stage(p), p, a, d, risk))

    o = []
    o += [f"### Differential review — `{base}`…`{head}`", "",
          f"{len(commits)} commit(s), {n} file(s) changed, **{high} HIGH-risk**. Suggested depth: **{depth}**.", ""]
    o += ["| File | +/− | Risk | Why |", "|---|---:|---|---|"]
    for p, a, d, risk, why in sorted(rows, key=lambda r: ({"HIGH": 0, "MEDIUM": 1, "LOW": 2}[r[3]], r[0])):
        o.append(f"| `{p}` | +{a}/−{d} | {risk} | {why} |")
    o.append("")
    o += ["#### Red flags", ""]
    flags = []
    for p, snip, subj in history:
        flags.append(f"- **Removed code introduced by a fix/security commit** — `{p}`: `{snip[:100]}` (was added in: {subj})")
    for p, snip in red_flags[:8]:
        if not any(snip[:60] in f for f in flags): flags.append(f"- Check-like line removed — `{p}`: `{snip[:100]}` — what attack did it stop?")
    for what, items in hazards.items():
        flags.append(f"- {what}: {len(items)} site(s), e.g. `{items[0][0]}`: `{items[0][1]}`")
    o += flags or ["- none found by the heuristics (not a clearance)"]
    o += ["", "#### Blast radius (callers of changed functions)", ""]
    o += [f"- `{n_}` in `{p}` — {c} reference(s){' — **wide**' if c >= 20 else ''}" for n_, p, c in blast[:10]] or ["- no widely referenced changed functions detected"]
    o += ["", "#### Test coverage of the change", ""]
    if src_changed and not tests_changed:
        o.append(f"- **{len(src_changed)} source file(s) changed and no test file changed** — raise severity of findings here by one level until covered.")
    o += [f"- changed fn not mentioned by any test: `{n_}` (`{p}`)" for n_, p in gaps[:12]] or ["- every changed fn is mentioned by some test (mention ≠ coverage; read the assertions)"]
    o += ["", "#### Reading order", "", "Definitions → implementations → adapters → wiring/config → tests, grouped by feature module:", ""]
    for grp in sorted(order):
        o.append(f"- **{grp}**")
        for st, p, a, d, risk in sorted(order[grp]):
            o.append(f"  1. `{p}` (+{a}/−{d}, {risk})")
    o += ["", "#### Questions for the author", "",
          "- Which invariant did the removed checks protect, and where is it enforced now?",
          "- What input reaches each added `unwrap`/cast/allocation, and who controls it?",
          "- Which test fails if the riskiest change is reverted?", ""]
    text = "\n".join(o)
    if out == "-":
        print(text)
    else:
        with open(out, "w") as f:
            f.write(text + "\n")
        print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
