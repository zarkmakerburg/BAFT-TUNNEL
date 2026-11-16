# ADR-0033: Panel HTTPS front end (domain or IP, certificate on the Linux host)

Status: accepted (2026-11-16)

## Context
The agent/panel is a non-root, loopback-only service that checks the `Host` header (DNS-rebinding guard) and authenticates with a bearer token. Remote use required an SSH tunnel. The owner wants the installer to accept a domain or an IP, get a TLS certificate on the host, and finish with an HTTPS link and the login details.

## Decision
- A TLS terminator, **nginx**, listens on 443 (`--https-port` to change) and proxies to the loopback agent. The agent is unchanged: still non-root, still loopback-only, no `CAP_NET_ADMIN`, no `CAP_NET_BIND_SERVICE` (nginx's master binds the port, as for any web server). nginx rewrites `Host` to the agent's loopback address so the rebinding guard stays on; nginx itself answers 421 for any other `Host`, so the public name/IP is the only accepted one.
- **Domain** -> Let's Encrypt via certbot webroot (HTTP-01, port 80 required); renewal by the distribution's `certbot.timer` plus a deploy hook that reloads nginx; the installer fails if the timer is not active and runs `certbot renew --dry-run`. HSTS (1 year, no includeSubDomains/preload) is sent only for a real certificate.
- **IP** -> no public CA is assumed for IP literals on the Ubuntu 22.04 baseline (certbot 1.21 cannot request them), so the host generates its **own CA + leaf**: P-256, CA name-constrained to exactly that IP (or domain with `--self-signed`), leaf 397 days, daily systemd timer renews the leaf when under 30 days. The installer prints the CA and leaf SHA-256 fingerprints and how to trust/pin; the browser warning is stated, never hidden. Revisit when an ACME IP-certificate client is available in the baseline.
- No silent downgrade: any failure (port in use, DNS, ACME, `nginx -t`, smoke test) restores the previous nginx config, removes files created by the run and exits non-zero. The agent stays loopback-only.
- Login: the panel's credential is the existing 0600 API token. At the end it is printed once, in a framed block, to a terminal only (never when stdout is not a TTY, never to journal/logs; later runs say where to read it unless `--show-token`). The agent has no user accounts or token rotation today, so forced rotation is not possible; recorded as a follow-up.
- nginx access log omits query strings and headers (`$uri` only); rate limit 20 r/s per IP (burst 60).
- Idempotent: re-running reuses a valid certificate; the config file is `/etc/nginx/conf.d/baft-tunnel-panel.conf` (backed up, restored on failure). `panel-https.sh --remove` removes the front end (certs kept).
- The firewall is not touched (ADR-0030); a hint is printed if `ufw` is active.

## Evidence levels
CI sandbox only: own-CA issuance, name constraint, auth pass-through, foreign-Host refusal, idempotence, rollback and nginx config for the domain path are tested with `scripts/test-panel-https.sh` (certbot and systemctl shimmed). A real Let's Encrypt issuance and renewal is **not** verified until run on a host with a public domain.
