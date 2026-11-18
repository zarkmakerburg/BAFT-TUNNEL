# ADR-0052: server location and reachability probes when adding a server

Status: **Proposed** (2026-11-18). Owner requirement (zark): when the user enters a server IP, the panel detects the country automatically and shows the server's health from several probes, including probes inside Iran in different cities. Extends ADR-0051 (add server), ADR-0045 (Fleet), ADR-0050 (pairing). Nothing here is implemented beyond the pure guard module named below; nothing is LAB-verified.

## Rules that apply to every part

- **Unknown, never guessed.** A value that was not measured is shown as Unknown with the reason (no database, probe disabled, probe timed out). No placeholder country or latency.
- **Only public targets.** The address the operator typed is checked before any lookup or probe: loopback, private, link-local, CGNAT, ULA, multicast, documentation, unspecified, broadcast and cloud-metadata ranges are refused (IPv4 and IPv6, including IPv4-mapped IPv6). A DNS name is resolved once and the checked IP is the only one used (ADR-0051 H2).
- **Sharing an IP with a third party is opt-in.** Off by default, per installation, with plain text in Persian and English saying which service receives the address, that it learns the server exists, and that the result is cached locally. The panel works fully with it off. The feature that contacts third parties ships as Preview.
- **Cache and rate limit.** Results are cached per IP (country 24 h, probe results 10 min), with a global budget (default 10 lookups per hour, 3 per target per hour) and one active probe run. A refresh button obeys the same budget.
- **No secret leaves.** Only the IP (and port for a TCP reachability check) is sent, never tokens, codes or names.

## Decision

### 1. Country (geo): offline first
The default source is an **offline GeoIP country database** read from a local file (for example a DB-IP or GeoLite2 country file the operator or installer places in the state directory, license and checksum documented). No network, no IP sharing. If the file is absent the panel shows Unknown and a link explaining how to enable it. An optional online lookup is a separate opt-in (see 3) and is never used silently.

### 2. Suggestion for the first setup question
`IR` suggests "Iran"; any other known country suggests "Abroad"; Unknown gives no suggestion. It is a pre-selection the user can change, never a lock, and the panel states the source ("from offline database"). Cloud and VPN IPs can be mislocated, so the text says "suggested".

### 3. Health and reachability, by source (best first)
1. **From this panel host** (always available, no third party): TCP connect to the service port and, after enrollment, the existing Integrity/Health tiers. Labelled "measured from this server".
2. **From our own enrolled Iran nodes** (preferred for Iran views, no third party): after Fleet F3, a node may run a typed read-only job `probe.tcp` against exactly the target IP and port the operator entered (same public-target guard on the member, bounded timeout, one attempt). The city comes from an operator-set label on the node and is shown as such, never inferred. Until F3 this source shows "No Iran node enrolled" (Unknown).
3. **External probe networks with Iran vantage points** (owner decision 2026-11-18: use them; consent-gated, Preview). Providers checked against live responses on 2026-11-18:

   | Provider | Iran vantage | Check type for a service port | Limits and notes |
   |---|---|---|---|
   | check-host.net | 8 nodes: Tehran x4, Shiraz x2, Isfahan, Qom (`GET /nodes/hosts`) | `check-tcp` host:port, `check-ping` | Public JSON API, no key. Returns `{"error":"limit_exceeded"}` when one client IP asks too often (hit during testing), so every call must tolerate it and show Unknown. Results are polled with `check-result/<id>`; a node still working returns null. |
   | Globalping (globalping.io) | 8 probes, all Tehran (several ASNs) | `ping` (ICMP), `http`, `dns`, `traceroute`, `mtr`; **no TCP type** | Public JSON API, anonymous limit 250 credits/hour (3 probes cost 3). ICMP failure is not proof a TCP service is down; the panel says so. |
   | ipcheck.ing | not verified | not verified | Returned HTTP 403 to automated requests and no documented API was found; not used until its API and terms are confirmed. Listed as a candidate only. |
   | RIPE Atlas | not verified | | Needs an account and credits; deferred. |

   Design consequences: check-host is the only verified source with several Iranian cities and TCP-port reachability, Globalping adds independent ICMP rows from Tehran networks, and both are per-source tables. A source that errors, is rate limited or times out contributes Unknown rows (never zero, never a copied value). Cities shown are the provider's own labels. Each provider is a separate adapter behind the same consent gate; the user sees the provider names and that the target IP (and port for TCP) is sent to them. Provider terms of service must be read and linked in the privacy text before the switch is enabled by default anywhere (it stays default-off). Request builders restrict node ids to `ir<digits>.node.check-host.net`, and every response field is sanitised and bounded before display.

Probe results show reachability and latency only. They say nothing about tunnel readiness (ADR-0045: connection freshness is not readiness).

### 4. Panel
In "Add server": after the address is entered, the card shows Country (with source), Suggested role, "From this server" reachability, and a per-city table for Iran probes with Unknown rows where not measured. The opt-in switch for external probes sits in Advanced with the privacy text. Persian RTL first.

## Staged plan
1. This ADR (review of external providers named in 3.3 is a follow-up).
2. `src/target_guard.rs`: pure public-target check, role suggestion from country code, result cache with TTL and budget; unit tests (done with this PR).
3. Panel card using offline DB and the host-side TCP check; Unknown states.
4. Iran-node probe job with Fleet F3; per-city table.
5. Pure adapters (`src/probe_adapters.rs`: request builders and response parsers, tests on real response shapes), done with this PR; then the HTTP client behind the consent gate (timeouts, size caps, cache, budget), default off, Preview until verified against live responses from a real run.

## Out of scope
No scanning of ports or ranges, no probing of addresses the operator did not type, no storing of probe history beyond the cache, no firewall or route change.
