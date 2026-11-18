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
3. **External probe networks with Iran vantage points** (opt-in, Preview): a public probe service that offers nodes in several Iranian cities. Candidates to verify before any code: check-host.net, Globalping, RIPE Atlas (needs account and credits). Each candidate must be checked for current Iran coverage, terms, rate limits and data retention in a review; none is chosen here. Results are shown per city and per source, with the probe's own timestamp. A provider that cannot be reached shows Unknown, not zero.

Probe results show reachability and latency only. They say nothing about tunnel readiness (ADR-0045: connection freshness is not readiness).

### 4. Panel
In "Add server": after the address is entered, the card shows Country (with source), Suggested role, "From this server" reachability, and a per-city table for Iran probes with Unknown rows where not measured. The opt-in switch for external probes sits in Advanced with the privacy text. Persian RTL first.

## Staged plan
1. This ADR (review of external providers named in 3.3 is a follow-up).
2. `src/target_guard.rs`: pure public-target check, role suggestion from country code, result cache with TTL and budget; unit tests (done with this PR).
3. Panel card using offline DB and the host-side TCP check; Unknown states.
4. Iran-node probe job with Fleet F3; per-city table.
5. External provider adapter, default off, after provider review.

## Out of scope
No scanning of ports or ranges, no probing of addresses the operator did not type, no storing of probe history beyond the cache, no firewall or route change.
