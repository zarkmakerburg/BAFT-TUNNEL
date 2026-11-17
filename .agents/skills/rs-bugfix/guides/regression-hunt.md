# Regression hunt — what changed between "worked" and "broken"

> **rs-bugfix · guide** — host-neutral

## 1. Pin before and after

Record the **good** commit/tag and the **bad** commit (`git rev-parse`). Work in a clean worktree for each (`git worktree add ../good <sha>`), or one worktree you move — and remove them afterwards (rs-cleanup: each has its own `target/`).

## 2. Make the oracle automatic

A repro that exits non-zero on the bug:

```bash
# oracle: succeeds on good, fails on bad
cargo test -q -p mycrate repro_upload_escape        # or a script that runs the request and checks the response
```

## 3. Bisect

```bash
git bisect start <bad> <good>
git bisect run cargo test -q -p mycrate repro_upload_escape
git bisect reset
```
If some commits do not build, `git bisect skip`; if tests need a service, make the oracle start it (containers: label and remove them, rs-cleanup).

## 4. Understand the culprit

`git show <sha>`; was it a deliberate behaviour change, an accidental invariant break, or a dependency bump (`Cargo.lock` diff — `cargo update` changes can regress)? Run `diff-review.py --base <sha>~1 --head <sha>` (rs-review) for risk, removed checks, blast radius.

## 5. Decide

| Finding | Action |
|---|---|
| accidental regression, still in production | revert the commit or hotfix (rs-bugfix table); add the repro test |
| dependency regression | pin/downgrade (`cargo update -p x --precise`), report upstream, add a `deny`/test guard |
| deliberate change with unintended effect | fix forward at the invariant; document |
| not a regression (always broken) | normal bugfix loop |

## 6. Regression campaign over a PR/range (optional)

For a risky merged range, list the candidate regressions from the diff (changed behaviours + their callers), reproduce each on before/after builds, **adjudicate** (evidence required: failing on after, passing on before, same stated cause), and report only confirmed ones with the repro and the commit — exclusions listed with reasons. The reproduction on both sides is what separates a regression from a bug that was always there.
