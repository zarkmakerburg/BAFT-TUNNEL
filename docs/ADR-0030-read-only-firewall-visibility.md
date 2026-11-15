# ADR-0030: Firewall visibility without CAP_NET_ADMIN (roadmap B3, P1-7)

Status: accepted (2026-11-15). Supersedes the first draft of this ADR (a root read helper), which the
owner rejected in the PR #210 review: no new BAFT-TUNNEL component, including a root helper, may be
granted `CAP_NET_ADMIN`.

## Context

Preflight's `firewall` check is Unknown. Operators would like to know whether the listener's TCP port
is reachable inbound. An unexplained nftables address-set change was also seen on one LAB host (S10).

## Findings (measured 2026-11-15, Ubuntu-class host with nft 1.0 / iptables 1.8.10 nf_tables)

Listing live rules is a netlink operation that the kernel gates on `CAP_NET_ADMIN`. Run with that
capability removed from the bounding and inheritable sets, even as uid 0:

- `nft list ruleset` → `Operation not permitted (you must be root)` / `cache initialization failed`
- `iptables-save` → `Could not fetch rule set generation id: Permission denied`

Capability-free surfaces expose only table and target *names* (`/proc/net/ip_tables_names`,
`ip_tables_targets`) and connection tracking, never rules or sets. So a reliable, live
firewall observation without `CAP_NET_ADMIN` does not exist on the Ubuntu 22.04 baseline.

## Decision

1. **The firewall state stays `Unknown`** in Preflight, Diagnostics and the capability catalog. The
   agent, the panel, the tunnel and every helper stay free of `CAP_NET_ADMIN`. No code changes.
2. **Rejected:** (a) a root path-unit helper holding only `CAP_NET_ADMIN` (the first draft: it
   moves the capability, it does not remove the risk); (b) giving the agent the capability;
   (c) parsing `/etc/nftables.conf` or ufw files, which describes configuration, not live state, and
   would invite a false "pass".
3. **What the product says instead:** Unknown, with the manual remediation "open the tunnel port on
   the listening server's firewall by hand". The operator-visible evidence of reachability remains
   `peer_reachability` (an actual TCP connect from the connecting side) and the end-to-end tiers,
   which observe the result a firewall would block, without reading any rule.
4. **Reopening** requires a new owner decision that accepts a capability-holding component, or a
   capability-free observation method (for example a future kernel interface), plus its own ADR and
   a real-host acceptance run.

## S10 (address-set drift)

Without reading sets, BAFT-TUNNEL cannot detect or attribute the drift, and it must not be assumed
to cause it: no BAFT-TUNNEL component writes firewall state (Apply never changes a firewall; receipts
say so). S10 stays an owner/ops investigation (O2): record the host's set contents with the host's
own admin tooling before and after each LAB stage. A clean-firewall claim stays unavailable.

## Not claimed

No firewall inspection, no verdict on inbound reachability from a firewall, no statement that the
LAB firewall is unchanged.
