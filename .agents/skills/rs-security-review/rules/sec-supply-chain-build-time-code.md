# sec-supply-chain-build-time-code

> Dependencies run code on your machine at build time: build.rs, proc-macros, git deps, patches
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Cargo executes `build.rs` scripts and procedural macros of **every** dependency (and transitive dependency) during `cargo build`/`check`/`test`/rust-analyzer — with your user's privileges, network and environment variables (CI secrets included). A compromised or typosquatted crate is code execution on developer laptops and CI runners before a single line of your program runs.

## Gates

- A new or updated dependency with `build.rs`, `proc-macro = true`, `links = …`, or native build steps.
- Git/path dependencies, `[patch]`/`[replace]` entries, wildcard versions, crates with few owners, recent ownership changes, abandoned or yanked versions, name look-alikes.
- CI that builds untrusted PRs with secrets available.

## Do

| Control | How |
|---|---|
| know what runs at build time | `cargo metadata` → targets of kind `custom-build` and `proc-macro`; `scripts/dep-risk.py` lists them (rs-release) |
| reproducible, reviewed lockfile | commit `Cargo.lock`; build with `--locked`; review lockfile diffs like code |
| policy gate | `cargo deny check` (advisories, licenses, bans, **sources** — no unknown registries/git), `cargo audit` |
| trust decisions | `cargo vet` (audits shared across teams) or a written allow-list for crates with build scripts |
| source of truth | `cargo vendor` + `--offline` for sensitive builds; mirror registry in CI |
| sandbox the build | build in a container/devcontainer without secrets and with no network after fetch (rs-cleanup `guides/devcontainer.md`); CI: no secrets on untrusted PR builds |
| minimise surface | prefer crates without build scripts / proc-macros when equivalent; disable default features |
| ongoing | scheduled `cargo audit`/`deny` (new advisories appear between commits); watch yanks and ownership changes |

Your own `build.rs`: keep it minimal and deterministic; no network, no writes outside `OUT_DIR` (rs-structure `proj-build-rs-minimal`).

## See Also

- [rs-release](../../rs-release/SKILL.md)
- [rs-reuse-first reuse-evaluate-candidate](../../rs-reuse-first/rules/reuse-evaluate-candidate.md)
