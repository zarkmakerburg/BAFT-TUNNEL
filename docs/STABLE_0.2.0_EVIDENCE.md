# Stable 0.2.0: release candidate evidence and release notes

Candidate: signed prerelease `v0.1.0-rc.12`, built from main `76c12cb95e68384146ace84c437c91c701a60e02` (includes the new dashboard, PR #253, and the dead-link detection fix, PR #251).
Binary SHA-256 `4c922d02da6c8465c130638ab5b71caf66af5adc974fafb1966f81cfce829136`. Stable `v0.2.0` is to be tagged from exactly this commit.

Evidence levels follow the project rule: CI ≠ VM ≠ LAB ≠ Production. Everything below is **real two-host LAB** (Iran ↔ abroad, Ubuntu 22.04) unless marked otherwise. Nothing here is production evidence.

## Gate items on the rc.12 binary

| Item | Run | Result |
|---|---|---|
| S2 cross-version update rc.11 → rc.12 + 3600 s soak | [43744552137](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43744552137) | Accepted under the outage rule (see below). 272 rounds, 1 failed probe at 18:57:37Z, 0 restarts, RSS 7652→7748 KiB (public) / 7392→7560 KiB (service), public fd 12→13 (one-off +1), service fd 12→12. The workflow conclusion is `failure` because the stage exits on any lost probe; its post-soak update and revocation steps therefore did **not** run in this run (covered by the S8/S9 run). |
| Outage proof for the lost probe | forensics [43754264737](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43754264737) | Iran host: tunnel TCP timeout (os error 110) 18:57:37 and 18:58:01; reconnect at 18:58:13 failed with `remote connect timeout` (SYN unanswered); session re-established 18:57:39 and 18:58:15 with no command; no kernel messages. |
| S3 agent service, lifecycle and one real reboot of both hosts | [43751162457](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43751162457) | PASS. Both hosts back in about 57 s; agent and tunnel returned on their own; data 64 KiB + 2 MiB after the reboot. Host drift on abroad (2 lines, unrelated to BAFT-TUNNEL, see S10). |
| New dashboard in a real browser on the LAB (same S3 run, `LAB_PANEL_BROWSER=1`, protocol v2) | 43751162457 | PASS, 19 of 19 checks: integrity probe from Diagnostics shows Passed and the API integrity tier is `pass`; Restart, Stop and Start through the confirm dialog with receipts matching the unit state; token never shown; no 5xx and no CSP violation. |
| S8 live CRL reload and S9 measured Integrity (protocol v2), rc.11 → rc.12, 300 s soak | [43752502531](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43752502531) | PASS. Revoked peer refused without a restart (serve PID unchanged); Integrity on the forward node `pass` 4 of 4 with SHA-256 match; soak 23 rounds, 0 failures, fd flat, 0 restarts. |

### Outage rule (coordinator decision, delegated by the owner, 2026-11-15)

A soak probe failure is accepted only when the host journals prove an external path outage in that window (tunnel TCP timeout plus an incoming SSH reset or an unanswered SYN on the same host), with automatic recovery, 0 restarts and flat fd/RSS. Any other failure fails the gate. Accepted-outage runs are listed in the release notes.

## Earlier context (not on the rc.12 binary)

- Probe-loss root cause: on 2026-11-15 the Iran host's network path dropped for 30–60 s at 07:16Z, 11:56Z, 15:27Z, 17:47Z and 18:57Z; each matched tunnel TCP timeouts, unanswered reconnects and (for most) incoming SSH resets on the Iran host, with 0 process restarts.
- PR #251 bounds dead-link detection at about 20 s (TCP_USER_TIMEOUT 20 s, TCP keepalive, yamux keepalive 5 s) instead of 30–45 s. CI and LAB (rc.11, rc.12) evidence; an in-flight request during an outage still fails.

## Release notes: known items

1. **Short outages on the operator's path still drop in-flight requests.** The tunnel detects a dead link in about 20 s and reconnects without a restart; requests in flight at that moment fail. Accepted-outage soak run: 43744552137 (1 of 272 probes).
2. **Integrity probe needs protocol v2 on both nodes.** `setup init` defaults to protocol v1; with v1 the Integrity tier stays Unknown (ADR-0028, no silent downgrade). One probe start per 60 s per link, so the second node probing right after the first reports `rate_limited`.
3. **Not verified in a browser on the LAB:** panel pairing, PKI enrollment (CSR/certificate upload) and BAFT guided setup in the new dashboard. These flows are LAB-proven through the agent API on earlier builds (panel pairing run 42779915532, D2 run 42996224181, B4 run 43644908151) and keep the same code paths; PKI enrollment has local browser QA with a mocked agent; pairing and the BAFT wizard have no browser run on the new dashboard.
4. **Open Low/Medium dashboard items** from the independent review, listed in `docs/PANEL_REDESIGN_PARITY.md` (hardcoded capability pills, `|| 0` KPI defaults, colour-only chart legend, small chart text on narrow cards, drawer/RTL details, lazy toast live region, language switch rebuilds the shell).
5. **Host drift on the abroad LAB host** (unrelated firewall/listener changes during runs) stays an operational note (S10), not a product finding.
6. Reverse and BAFT remain Preview; Ready stays Unknown by design; no CAP_NET_ADMIN in any component; firewall status stays Unknown (ADR-0030).

## Review of the delta since the pre-Stable audit

Delta `8322424..76c12cb` in `src/`, `panel/`: PR #251 (socket options via unprivileged `setsockopt`, size-checked, errors only logged; no new capability) and PR #253 (dashboard). Focused manual security review on 2026-11-15: the panel builds DOM only through `BAFT.h` (`textContent`, `addEventListener`, `setAttribute` from code-controlled keys); no `innerHTML`, `eval` or string handlers; links are code constants or `blob:` downloads; the new `ui.js` is served from the fixed asset list under the existing CSP (`script-src 'self'`, `default-src 'none'`); the token stays in `sessionStorage`. No Critical/High/Medium finding. The dashboard thread's independent review items are listed in known item 4.
