# 21st.dev CLI — BAFT-TUNNEL

## Purpose and boundary

The 21st.dev CLI is a developer-side discovery and design tool for researching UI components, themes, and implementation patterns. It is **not** a runtime dependency of BAFT-TUNNEL.

The local Agent panel is intentionally dependency-free vanilla HTML/CSS/JavaScript, served under a strict self-hosted Content Security Policy. Use `21st search` and `21st get` to inspect ideas; do not run `21st add` against the local panel. Port a pattern manually only after checking its license, dependencies, accessibility, Persian RTL/English LTR behavior, CSP compatibility, security, and bundle footprint. The future Central Fleet Console is a separate, gated surface; its possible React/TypeScript direction does not authorize migrating the local panel.

The root `DESIGN.md` and `AGENTS.md` remain authoritative for product identity, architecture, visual tokens, and adoption limits. The CLI-generated `.21st/design.json` and `.21st/DESIGN.md` provide an additional project-aware context layer. The current CLI does not automatically infer all product/stack/token details from this Rust + static-panel repository, so do not treat empty auto-detected fields as a statement that the product has no design system.

## Local developer setup

Install the CLI globally (the repository has no Node package manifest at its root):

```bash
npm install --global @21st-dev/cli@1.18.0
21st login
21st whoami
21st init --design-context --refresh
```

`21st login` opens a browser and saves the local sign-in state outside the repository. Never copy that state into the repository, CI, issue comments, logs, or screenshots. Use `21st logout` to clear the local sign-in when needed.

After changing the root design instructions, regenerate the context and verify it:

```bash
21st init --design-context --refresh
21st init --design-context --check
```

The refresh command rewrites generated context from the current checkout and may replace manually edited generated fields. Keep durable product design decisions in root `DESIGN.md` / `AGENTS.md`, review the generated diff after refresh, and do not hand-edit generated metadata as a substitute for updating those source documents.

Useful commands:

```bash
21st search "accessible dashboard sidebar" --limit 10
21st get <search-result-id>
21st init --design-context --check
```

## CI and non-interactive scripts

CI must **not** run `21st login`. Store the API key as a GitHub Actions secret named `API_KEY_21ST` (repository-level or a dedicated tooling environment). Do not put it in the `baft-tunnel-lab` environment, which is reserved for the two-host LAB.

Pass the key explicitly:

```bash
test -n "${API_KEY_21ST:?Set the API_KEY_21ST CI secret}"
21st search "accessible dashboard sidebar" --limit 10 --api-key "$API_KEY_21ST"
```

Or expose the same secret as the `API_KEY_21ST` environment variable; the CLI recognizes it without an interactive login:

```yaml
- name: Run 21st.dev CLI
  env:
    API_KEY_21ST: ${{ secrets.API_KEY_21ST }}
  run: |
    test -n "$API_KEY_21ST"
    21st search "accessible dashboard sidebar" --limit 10
```

Never echo the key, enable shell tracing around commands that receive it, hard-code it, or pass it as a committed workflow literal. If the secret is absent, fail closed rather than attempting browser login.

## Verified setup state

- CLI package: `@21st-dev/cli@1.18.0`; executable: `21st`.
- Local sign-in: verified with `21st whoami` as the project owner's 21st.dev account.
- API authentication: verified with a successful `21st search` request.
- Design context: `.21st/design.json` and `.21st/DESIGN.md`; `21st init --design-context --check` reports current after refresh.
- CI secret: **not configured by this setup**; the owner must add `API_KEY_21ST` in GitHub Actions settings before any workflow uses it.
