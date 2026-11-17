# Tunnel creation flow audit (2026-11-17)

Evidence level: code and docs read at `main` `5cf9db8` (no LAB, no live run). Trigger: owner installed
v0.2.2 and could not create a sample tunnel. Sources: `README.md`, `docs/quickstart.en.md`,
`panel/pages.js` (Wizard), `src/setup.rs` (init/join), `src/pki_helper.rs`, `src/agent.rs` routes,
`docs/ADR-0012/0014/0026/0027`.

## Current real flow, tcp-tls Direct (the supported path)

| # | Step | Who does it | Manual burden |
|---|---|---|---|
| 1 | One-line install on host A, then host B (`--register NAME`) | user, 2 SSH sessions | name per host; installs agent only, **starts no tunnel** |
| 2 | Open each panel: SSH `-L 18080`, or `--domain/--ip` HTTPS | user | read root-only token file; two browser sessions |
| 3 | Wizard on **each** host separately: direction, role, node id, peer id, tunnel id, service port, tunnel port, peer IP, peer cert name, own cert name, instance, directory, protocol version, engine | user | ~14 fields twice; both sides must match exactly; "cert name" is undefined jargon |
| 4 | PKI: create CSR on each host | user | copy CSR text out of the panel |
| 5 | Sign on a **third offline machine** with `scripts/pki/baft-pki.sh init-ca` / `issue-node` (needs repo checkout, openssl, shell) | user | the step even experts fail; CLI args must match the panel exactly |
| 6 | Download leaf + CA cert + CRL, upload 3 files into each panel | user | 6 file moves; manual confirm checkbox |
| 7 | Preflight, preview, Init on each host | user | plan review of raw JSON |
| 8 | Export bundle on A, move to B (and vice versa) by hand | user | copy/paste or file transfer between machines |
| 9 | Compare checksums by phone, paste, Join on each host | user | out-of-band channel needed |
| 10 | Start on each host, run diagnostics | user | two more clicks, panel reloads |
| 11 | CRL expires (30 days); renew offline and push by SSH | user | recurring, otherwise tunnel fails closed |

Quick-start doc also warns that the menu "Pair the two nodes" shortcut creates noise-tcp (Experimental),
so the easy-looking path is the wrong one.

## Where users get stuck (verified)

1. **Offline CA (steps 4-6, 11).** Required by ADR-0012/0014 (CA key never on a node). `scripts/pki/baft-pki.sh`
   is a repo script; nothing in the panel can run it. No online enrollment exists (ADR-0014 defers it).
2. **Duplicate, hand-synchronised configuration (step 3).** `PlanRequest` needs both node ids, tunnel id, ports,
   server name and instance on both hosts. Any typo fails at Join as a "mismatch".
3. **Manual bundle transfer + out-of-band checksum (steps 8-9).** Only mechanism that binds the peers.
4. **No single "connect" action.** Install, pair, Start are separate; the tunnel is never started by install.
5. **Jargon and raw JSON** in Preview; remedies not tied to a plain-language fix.
6. **Recurring manual CRL renewal** makes even a successful tunnel decay.

## Target

User chooses only: name, direction (Direct, Reverse = Preview), which service port to expose. Everything else is
generated, and no file is moved by hand. See ADR-0050 for the design and the staged plan.
