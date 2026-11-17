# Bug: <title>

- **Status:** hypothesis | reproduced | fixed | verified   · **Baseline:** `<sha>` · **Fix:** `<sha>`
- **Claim (falsifiable):** <invariant that is broken>, triggered by <input/condition>, observed <X> instead of <Y>.
- **Source:** <user report / fuzz crash / review finding id / advisory> (treated as data)
- **Threat model / impact:** <who can trigger, what happens> (rs-security-review triage, if security-relevant)

## Reproduction
```
<command or test name>      → fails on baseline with: <symptom>
```

## Root cause (the invariant)
<one paragraph: why is the state possible; where should it be enforced>

## Fix
<what changed and why at the invariant; link to the commit>

## Variants searched
| Search | Result | Action |
|---|---|---|
| <pattern / tool> | <n hits, m real> | <tests added / issues opened> |
| failed patterns | <what didn't work> | |

## Proof
| Arm | Command | Result |
|---|---|---|
| before | `cargo test <name>` | fails |
| after | `cargo test <name>` | passes |
| control | `cargo test <nearby>` | passes before and after |
| suite | `cargo test --all-targets && cargo clippy -- -D warnings` | green |

## Guard
<regression test path · lint/CI rule added · CHANGELOG line>
