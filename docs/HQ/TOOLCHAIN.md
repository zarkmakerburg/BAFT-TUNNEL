# Connected Toolchain

Use tools as scoped executors. The GitHub repository and Issue #2 remain the record; a connected tool's presence does not imply that every account, host, project or permission is available. Before each operation, inspect the relevant live state and use the narrowest supported action.

| Tool / integration | Use it for | Evidence to retain | Limits and safe-use rules |
|---|---|---|---|
| GitHub connector / `gh` CLI | Repository metadata, branches, commits, Issues, PRs, reviews, workflow runs, file changes and PR creation | Repository/ref, exact SHA, issue/PR URL, workflow/run ID and observed status | GitHub is authoritative for repo state. Preserve other workers' branches and PRs. Do not expose tokens. `gh` availability on one machine does not imply availability in every session. |
| Remote Desktop Commander (Mac) | Authorized Mac filesystem inspection, shell commands, local build/test, git operations and access to a dedicated checkout | Device identity (non-secret), working path, command, exit code, relevant output, branch and SHA | Operates on the selected connected device. Inspect the target path and current checkout first. Never use the stale `~/BAFT-TUNNEL` checkout without verifying it; use an isolated workspace. Do not print environment secrets or mutate persistent LAB/production hosts. |
| CodSpeed | Run and compare benchmark suites; inspect benchmark results and flamegraphs | Benchmark run ID, commit SHA, benchmark name, instrument/mode and comparison baseline | Benchmark simulation is not real network/LAB evidence. System-call-heavy benchmarks may need wall-time measurement. Do not edit another worker's CodSpeed PR/branch. |
| Context7 | Resolve current library identifiers and retrieve version-relevant API/framework documentation | Library ID, version/context and relevant documentation reference | Documentation lookup is not proof that the local dependency version matches; inspect Cargo/package manifests and lockfiles. |
| Figma | Inspect design context/screenshots and work on UI/design-system artifacts when explicitly in scope | File/node reference, screenshot or design-context output, relevant component/variable mapping | Load the required Figma skill before write/design-context operations. Do not treat a mockup as implemented UI or acceptance evidence. |
| Git local tooling | Isolated clone/worktree, diff, static checks, tests, commits and branch comparisons | Exact command/output, clean/dirty state, base/head SHA | Check branch and workspace ownership before writes. Never reset/force-push to resolve uncertainty. |
| Optional project integrations | Notion/Linear for an explicitly requested external workflow; deployment/observability integrations only for an authorized task | External record ID and exact result | Not an alternate project board or source of truth. Do not create duplicate task systems. Deploy/production actions require their own gate and authorization. |

## Selection and evidence rules

1. Prefer the GitHub connector for GitHub-native reads/writes and the local terminal for local execution. Use one source to verify the same fact where possible; cross-check high-impact changes.
2. For performance work, use CodSpeed on the intended benchmark branch and exact commit; label simulated and native runner modes accurately.
3. For API/library uncertainty, consult Context7 and then validate against the versions pinned in the repository.
4. For UI tasks, use Figma only when design context or a Figma artifact is needed; pair design review with browser/device evidence for implementation acceptance.
5. Record exact identifiers and outputs, not screenshots or claims without provenance. Redact secrets before storing logs or links in public Issues/PRs.
6. If a tool is unavailable, disconnected or lacks permission, mark the step BLOCKED/UNKNOWN and state the smallest required access change. Never imply it was installed or activated by writing documentation.
