# Stable 0.2.2: release candidate evidence

Candidate: signed prerelease `v0.2.2-rc.3`, built from main `5dd0890cac9deddcac66c3105d3fbaf9164e391e` (includes the installer port-443 exit fix, PR #285).
Binary SHA-256 `5f4ef12063c9d8a7a55ce993380c84d42ec648ad8121d3c4dd02cea43ac54247` (baft-tunnel) and `e4f04837253f83b7e52bca29bd93f0e4b2bd627325a75dee37d93b1057513647` (baft-engine). Both are byte-identical to `v0.2.2-rc.2`; between rc.2 and rc.3 only `scripts/quick-install.sh` and its test changed.
Stable `v0.2.2` is to be tagged from exactly this commit.

Evidence levels: CI ≠ VM ≠ LAB ≠ Production. Everything below is **real two-host LAB** (Iran ↔ abroad, Ubuntu 22.04) unless marked otherwise. Nothing here is production evidence.

## Installer and HTTPS (owner path: domain → web address, web path, password)

| Item | Run | Result |
|---|---|---|
| rc.3, abroad host, terminal install, domain typed at the prompt, Let's Encrypt production | [43896859603](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43896859603) | PASS. Ends with `Link : https://DOMAIN/<web path>/`, the password, `Panel (HTTPS):` and the menu prompt (`terminal_link_password=1`); gate 404 without cookie; auth 200; certbot timer and renew dry-run; cleanup. |
| Fix check on the Iran host, same terminal path, installer from main, Let's Encrypt production, offline Sigstore verification | [43893761484](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43893761484) | PASS (`signature=offline`, `terminal_link_password=1`). |
| rc.2, Iran host, `--domain` | [43878101470](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43878101470) | PASS (offline signature, web path, 404 without cookie). |
| rc.2, abroad host, terminal install | [43879430384](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43879430384) | FAIL → bug found: after a successful HTTPS setup on port 443 the installer exited 1 before the summary and menu (`set -e` on a `$(test && echo)` assignment). Fixed in PR #285 with a regression test; rc.2 was therefore not promoted. |

Ubuntu 24.04 is covered only by the CI job `ubuntu-2404-pipe-install` (CI, not LAB).

## Upgrade rc.2 → rc.3 with 3600 s soak

| Run | Window (UTC) | Result |
|---|---|---|
| [43897433228](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43897433228) | 15:5x–16:45 | Soak gate failed, 5 probes (mostly 2 MB probes cut at 0.5–1 MB). |
| [43910396414](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43910396414) | 16:5x–17:55 | Soak gate failed, 7 of 210 probes; forensics [43934069639](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43934069639): `Connection reset by peer` on the abroad node while the Iran node timed out, sshd kex closes on both hosts. |
| [43945271865](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43945271865) | 20:00–21:10 | **Accepted under the outage rule.** 2 of 240 probes failed (21:03:33Z 64 KiB, 21:06:45Z 2 MiB); upgrade, rollback, guarded update, failed-update rollback, tampered-restore refusal and restore PASS; 0 restarts; fd and RSS flat. Forensics [43958046663](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43958046663): both nodes logged a yamux `Connection timed out (os error 110)` in the same second; incoming sshd kex connections on the Iran host closed at 21:05–21:06Z. The stage exits on any lost probe, so its post-soak reconnect and backup steps did not run in this run. |

The same binary (rc.2) passed the rc.1 → rc.2 3600 s soak (run [43879798413](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43879798413), 1 of 268 probes, accepted as an outage) and the rc.1 upgrade soak earlier on the same day.

### Decision (coordinator, delegated by the owner, 2026-11-16 21:15Z)

The 2026-11-15 outage rule applies to run 43945271865: the lost probes are accepted because host journals show the path outage, recovery was automatic, there were 0 restarts and fd/RSS stayed flat, and the binary is identical to rc.2. Stable 0.2.2 may be requested on `5dd0890`. The tag push still needs the owner's own approval.

## Known items

- Iran path, Tehran evening of 2026-11-16: sustained tcp-tls transfers were repeatedly reset after about 1 MB (Direct). Recorded as a research item in the roadmap (PR #286). No censorship-resistance claim.
- Real Let's Encrypt was verified on the LAB domains only; no production host.
