# Stable 0.2.1: evidence and release notes

Stable `v0.2.1` was published 2026-11-16 09:24Z on main `92a661fa5674ec7307d99fc6f5b1a9191e3fc900`, the same commit as the signed prerelease `v0.2.1-rc.1`
([release run 43817865343](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43817865343) for the rc, [43838910168](https://github.com/zarkmakerburg/BAFT-TUNNEL/actions/runs/43838910168) for Stable). The Linux binary digest is the same on both
(`sha256:1d34a77569f323d5bbf84853fc79bad12b0d395c057816c024e54a1961d46634`). The owner approved the Stable tag push in their own words.

Levels: CI ≠ VM ≠ LAB ≠ Production. Nothing here is production evidence. Reverse and BAFT stay **Preview**; `ready` stays Unknown by design.

## What changed since 0.2.0

| Item | Change | Evidence |
|---|---|---|
| R1 | Correct HTTP reason phrases on agent error statuses (PR #260) | CI unit test over every emitted status |
| R3 | HTTPS front end checked against a real ACME server: Pebble with unshimmed certbot in CI (PR #260); the advisory `certbot renew --dry-run` is bounded by `timeout 120` | CI (`ubuntu-2204-baseline`) |
| R5 | Panel Low/Medium items: no invented `0`/capability defaults, shape as well as colour in the status strip, toast live region, small-screen chart text (PRs #261, #262) | CI (`panel-ui`, axe) |
| R4 | First-run flows through the panel in a real browser on both LAB hosts: wizard, PKI CSR and certificate install, Prepare, bundle download, wrong and right checksum import, Join, Start (PR #263) | REAL-LAB, reported by the Stable/LAB thread on the rc.1 binary: BAFT 48/48, tcp-tls (Persian) 46/46 |
| S3 | Reboot of both hosts, agent and tunnel back on their own | REAL-LAB 19/19 on rc.1 |
| S2 | 0.2.0 → rc.1 upgrade with a soak of 60 minutes | REAL-LAB, clean |
| HTTPS | One-line installer with `--ip` (own CA, IP SAN, health 200, 401 without token, token 200, untrusted chain refused, agent loopback-only), then cleanup | REAL-LAB pass on the abroad host |

## Known items carried into 0.2.2 and later

- **Version string.** Every release up to and including v0.2.1 was built from `Cargo.toml` version `0.1.0`, so `baft-tunnel --version`, `/v1/status` and the panel report 0.1.0. Fixed on main for the next release (PR #273: `Cargo.toml` follows the release and `release.yml` refuses a mismatching tag). An update notice (ADR-0035) therefore treats 0.1.0 as an unknown installed version.
- **Real Let's Encrypt `--domain`.** Not covered by the 0.2.1 evidence. Run afterwards on the two LAB domains with the installer from main against the signed rc.1 package, and recorded with the 0.2.2 patch evidence.
- **Iran hosts and the Sigstore CDN.** The one-line installer of 0.2.1 verifies the signature online only; the Sigstore CDN answers 403 there. Offline verification (ADR-0034, PR #274) is in the 0.2.2 patch.
- **Panel certbot dry-run.** Without a terminal certbot's random sleep could print a false renewal warning; fixed in PR #271 (0.2.2).
- Open from 0.2.0: abroad-host address-set drift (S10) not attributed; in-flight requests fail during a short path outage; protocol v1 was still the `setup init` default (v2 is the default from 0.3.0, ADR-0028 amendment).
