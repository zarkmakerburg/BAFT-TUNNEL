#!/usr/bin/env python3
"""Regenerate the navigation pages of an rs-reviews folder.

  review-index.py [<rs-reviews-dir>]

Writes <dir>/README.md (all days, newest first) and <dir>/<day>/README.md (one page per day: reports,
verdicts, severity counts, fix progress, reading order). Reads the front matter of each report
(kind, scope, crate, verdict, critical/high/medium/low, commit) and counts `- [x]` / `- [ ]` boxes.
Safe to run any time; it only writes README.md files.
"""
import re
import sys
from pathlib import Path

root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
DAY = re.compile(r"^\d{4}-\d{2}-\d{2}$")
VERDICT_ORDER = {"Block": 0, "Fix before merge": 1, "Ship": 2}


def front(text):
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    d = {}
    if m:
        for line in m.group(1).splitlines():
            if ":" in line:
                k, v = line.split(":", 1)
                d[k.strip()] = v.strip()
    return d


def boxes(text):
    text = re.sub(r"<!--.*?-->", "", text, flags=re.S)      # template hints contain example boxes
    text = re.sub(r"```.*?```", "", text, flags=re.S)          # code samples too
    return len(re.findall(r"^\s*- \[[xX]\]", text, re.M)), len(re.findall(r"^\s*- \[[ xX]\]", text, re.M))


def num(d, k):
    try:
        return int(d.get(k, "0"))
    except ValueError:
        return 0


def row(path):
    t = path.read_text(errors="replace")
    d = front(t)
    done, total = boxes(t)
    d["_done"], d["_total"] = done, total
    d["_file"] = path.name
    return d


days = sorted((p for p in root.iterdir() if p.is_dir() and DAY.match(p.name)), reverse=True) if root.is_dir() else []
LEGEND = "Severity: 🔴 Critical (blocks) · 🟠 High (fix before merge) · 🟡 Medium (debt) · 🔵 Low (polish). Verdicts: **Block** · **Fix before merge** · **Ship**."

top = ["# Review reports", "",
       "Reports are grouped **by day**. Open the newest day, read its README, then the overview (or the report with the worst verdict).", "",
       LEGEND, "",
       "| Day | Reports | Worst verdict | 🔴 | 🟠 | 🟡 | 🔵 | Fixed |", "|---|---:|---|---:|---:|---:|---:|---|"]
for day in days:
    rows = [row(p) for p in sorted(day.glob("*.md")) if p.name not in ("README.md", "work-log.md")]
    if not rows:
        continue
    crate_rows = [r for r in rows if r.get("kind") != "overview"] or rows
    worst = min((r.get("verdict", "pending") for r in crate_rows), key=lambda v: VERDICT_ORDER.get(v, 3))
    done = sum(r["_done"] for r in crate_rows)
    total = sum(r["_total"] for r in crate_rows)
    top.append(f"| [{day.name}]({day.name}/README.md) | {len(crate_rows)} | {worst} | "
               f"{sum(num(r,'critical') for r in crate_rows)} | {sum(num(r,'high') for r in crate_rows)} | "
               f"{sum(num(r,'medium') for r in crate_rows)} | {sum(num(r,'low') for r in crate_rows)} | {done}/{total} |")
    out = [f"# Reviews — {day.name}", "", f"[← all days](../README.md) · [Agent work log](work-log.md)" if (day / "work-log.md").exists() else "[← all days](../README.md)", "",
           LEGEND, "",
           "## Reports", "",
           "| Report | Kind | Verdict | 🔴 | 🟠 | 🟡 | 🔵 | Fixed | Commit |", "|---|---|---|---:|---:|---:|---:|---|---|"]
    order = sorted(rows, key=lambda r: (r.get("kind") != "overview", VERDICT_ORDER.get(r.get("verdict", "pending"), 3), r.get("crate", "")))
    for r in order:
        name = r.get("crate") if r.get("kind") != "overview" else f"{r.get('scope', 'overview')} (overview)"
        out.append(f"| [{name}]({r['_file']}) | {r.get('kind', 'crate')} | {r.get('verdict', 'pending')} | {num(r,'critical')} | {num(r,'high')} | {num(r,'medium')} | {num(r,'low')} | "
                   f"{r['_done']}/{r['_total']} | `{r.get('commit', '')}` |")
    out += ["", "## How to read these", "",
            "1. **Overview first** (if present): verdict, what repeats across crates, the fix order.",
            "2. **Then the crate with the worst verdict**: each report opens with a TL;DR and three 'start here' links.",
            "3. Fix **phase by phase** (P1 → P6); tick the boxes in the crate report as you go — the *Fixed* column above recounts them (`python3 -I skills/rs-review/scripts/review-index.py <rs-reviews dir>`).",
            "4. Want to check the reviewer's work? [work-log.md](work-log.md) lists every step with its evidence."]
    (day / "README.md").write_text("\n".join(out) + "\n")
(root / "README.md").write_text("\n".join(top) + "\n") if days else None
print(f"indexed {len(days)} day(s) in {root}")
