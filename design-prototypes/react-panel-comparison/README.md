# BAFT-TUNNEL React panel — visual comparison

Three independent visual directions for the **existing local Agent panel**:

- **01 Weave:** dark navy/cyan, closest to the accepted local panel design language.
- **02 Clarity:** light, spacious, restrained product UI.
- **03 Command:** compact, high-density operations console with icon rail.

The page selector changes the same route in all three columns so the owner can compare each of the nine existing routes side by side. Persian RTL is the default; the language switch also exercises English LTR.

## Important scope and truth boundary

This is a static design prototype. It does not connect to the Agent API, does not contain real operational data, and does not implement tunnel operations or authentication. All values are deliberately unknown/empty and the prototype carries a visible preview-only warning. Do not deploy it as the product panel.

Routes represented: Overview, Tunnels, Servers, Monitoring, Setup, Diagnostics, Activity, Agent, Settings.

## Run locally

Requires Node.js 20+ on the development machine only:

```sh
npm install
npm run dev
```

Build check:

```sh
npm run build
```

The deployed product decision remains: React + TypeScript, static assets served by the Rust Agent; no Node.js runtime on target hosts. The selected design must be rebuilt against the existing authenticated API and accepted security/UX contracts before product migration.

No paid AI generation, credit purchase, or subscription is used.
