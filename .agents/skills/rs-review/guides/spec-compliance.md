# Spec-to-code compliance — does the code do what the document says?

> **rs-review · guide** — rs-skills original

Two artefacts disagree and the job is to find where: the document (RFC, design note, `gcl.yaml`, README guarantees, MCP spec section) says what the system should do; the code decides what it does. Every gap is a bug **or** a documentation fix — deciding which is the finding. Related rule: **the contract is the contract** — never narrow a published schema to match the code (rs-api-design `api-config-is-contract`).

## When

You have an authoritative document and the code that implements it: a policy definition (GCL) against its filter, a protocol section against a handler, a public API promise against the function. Not for code with no document (infer intent with `context-building.md` first).

## Procedure

1. **Split the document into individually checkable requirements** (a sentence with MUST/SHOULD/"never"/"always", a config option with its enum values, a table row). Number them `R1…Rn`.
2. For each, **find where it would be enforced** and read it, its callees **and its callers**. Follow every path, including error and deny paths.
3. Give each a verdict:

| Verdict | Meaning | Usually |
|---|---|---|
| `implemented` | holds on every path | fine |
| `partial` | holds on the paths anyone tests, fails on another | **the most serious** — it survives because nobody tried that path |
| `contradicted` | code does the opposite | bug or outdated doc — decide |
| `absent` | nothing enforces it | finding **only** with the `searched` record (patterns tried, results) — enforcement often hides in a layer/macro/caller you did not read |
| `stronger-than-spec` | code enforces more than documented | undocumented constraint; document it before someone removes it |
| `undecidable` | the requirement is too vague to check | a finding about the document |

4. **Reverse direction:** list behaviour the code has that no document mentions (extra headers written, properties set, options read) — the per-requirement pass cannot find these.
5. **Refute your own divergences.** Before reporting, try to knock each one down (a second read of the code; a second read of the document). Keep what survives; list what you could not verify.

## Severity is consequence, not distance from the text

A fail-open path in an authorisation filter outranks a MUST satisfied by different means; wording drift with no behavioural effect is a docs ticket.

## Record

A table `| Req | Text | Where enforced | Verdict | Evidence (file:line, searches) |` in the report's appendix. For gateway policies, include: every `gcl.yaml` property and enum value → code that reads it; every property/header the policy **writes** → the readers in other policies (rs-domain-gateway property contracts; `risk-scan.py` lists unmatched ones).
