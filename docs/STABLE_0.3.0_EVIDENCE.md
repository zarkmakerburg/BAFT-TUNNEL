# Stable 0.3.0: release candidate evidence (draft, not approved)

Candidate: signed prerelease `v0.3.0-rc.1`, built from main `69f43ed05b38f98320a74ceecae5c83d2fba391d` (Cargo version 0.3.0, PR #290).
Binary SHA-256 `dd260c584c0f5c1d851a88d78f6228162cc8d420a4d59be003e4d8160a3f79e6` (baft-tunnel) and `e4f04837253f83b7e52bca29bd93f0e4b2bd627325a75dee37d93b1057513647` (baft-engine, unchanged since 0.2.2).

**Status: Stable 0.3.0 is not accepted.** Decision (coordinator, delegated by the owner, 2026-11-17 09:22Z): it waits for a fix or an understood cause of the Direct long-lived-session stall described under Known items. No Stable tag exists for 0.2.2 or 0.3.0 yet; both tag pushes also need the owner's own approval and an admin machine.

Evidence levels: CI ≠ VM ≠ LAB ≠ Production. Everything below is **real two-host LAB** (Iran ↔ abroad, Ubuntu 22.04) unless marked otherwise. Nothing here is production evidence.

## What 0.3.0 adds over 0.2.2

| ID | Item | PR / ADR |
|---|---|---|
| O1 | Protocol v2 is the default for new pairs (BAFT stays v1) | #268, ADR-0028 amendment |
| O2 | Host tunnel list for several instances per host | #279, ADR-0036 |
| O3 | Offline update notice | #275, ADR-0035 |
| O4 | Certificate expiry warning in `/v1/status` | #270 |
| O5 | Secret-free support bundle | #272 |
| O6 | Reverse (tcp-tls) offered as **Preview** with a two-node integrity probe | #282, #283, #287, #288, ADR-0037 |
| O7 | Fuzz target for state-dir file readers | #281 |

Reverse stays Preview; no claim beyond the evidence below.

## LAB results on `v0.3.0-rc.1`

| Item | Run | Result |
|---|---|---|
| Reverse Preview (ADR-0037): built binary equals the signed rc.1 | [44031292339](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44031292339) | PASS. Integrity probe from both nodes; 30-minute soak, 132 rounds, 0 failed probes; SIGKILL and Apply restarts, backup/restore, update and bad-update rollback, revoked peer refused, uninstall. `cross_version_update=0` (not covered). |
| Iran host, terminal install, Let's Encrypt production, offline signature | [44037458129](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44037458129) | PASS. Link, web path and password printed. |
| Abroad host, Let's Encrypt staging | [44038004932](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44038004932) | PASS. |
| 0.3.0 features on the upgraded agents, both nodes | [44043045024](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44043045024) | PASS: `host_tunnels=1 update_notice_real=current update_notice_fixture=1 cert_warning=renew_soon support_bundle=1`. Not LAB-tested: panel display of these, and the host list with two real instances. |
| Upgrade rc.3 → rc.1 with soak | [44039365214](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44039365214), [44040564605](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44040564605), 44043045024 | **NOT PASSED.** Upgrade, rollback and the update rollbacks pass; the first 2 MB probe after the upgrade is cut at 0.64–0.92 MB with a yamux timeout on both nodes in all three runs. |
| Control: 0.2.2 binary (rc.2 → rc.3) on the same path | [44044543973](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/44044543973) | Same cut on every 2 MB soak probe while 64 KB probes pass. So this is **not a 0.3.0 regression**. |

An earlier upgrade run (44032181972) failed on a harness timing gap, fixed in #297; #299 moved the 0.3.0 feature checks to just after the agent upgrade.

## Known items

- **Direct long-lived sessions lose large transfers on the Iran path** (Iran dials abroad), on both 0.2.2 and 0.3.0-rc.1 since about 16:00Z on 2026-11-16: 2 MB probes stop at about 0.6–1 MB, yamux timeouts on both nodes, 0 restarts, automatic recovery. Reverse (abroad dials Iran) is clean on the same path. It does not fit the 2026-11-15 outage rule, so it blocks Stable. A product cause is not ruled out. Investigation is a separate thread (roadmap §9.5 research item, PR #286); measurement first, no workaround or censorship-resistance claim before it is measured.
- Panel display of the update notice, certificate warning and tunnel list is covered by CI/local browser only, not LAB.
- Real Let's Encrypt was verified on the LAB domains only; no production host.
- `cross_version_update` was not covered in the Reverse run.

## Before Stable 0.3.0 may be tagged

1. The Direct stall is fixed or its cause is understood and accepted by the coordinator, and an upgrade soak passes on the final candidate.
2. A new rc if any code changes; the Stable tag goes on exactly the commit of the last rc.
3. The owner approves the tag push in their own words on an admin machine.
4. Then bump the README one-line install pin (it currently names `v0.2.1`) in a separate PR and merge it only after the tag exists.
