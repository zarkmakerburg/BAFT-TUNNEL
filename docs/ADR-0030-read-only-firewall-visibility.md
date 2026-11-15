# ADR-0030: Read-only firewall visibility (roadmap B3, P1-7)

Status: accepted (owner mandate 2026-11-14: decisions delegated; architecture rule: no `CAP_NET_ADMIN` in the panel or agent).
Builds on ADR-0023 (root path-unit helper) and ADR-0026/0027 (bounded helper protocol).

## Context

Preflight's `firewall` check has always been Unknown: the agent (user `baft-tunnel-agent`) cannot
list nftables or iptables without `CAP_NET_ADMIN`, and giving it that capability is not acceptable.
Operators still need to know, before they start a tunnel, whether the listener's TCP port is
reachable inbound. An unexplained change of an nftables address set was also observed on one LAB host
(S10); a visibility tier must not be mistaken for the cause, and must help find it.

## Decision

1. **Same helper pattern as ADR-0023.** The agent writes `/run/baft-tunnel/control/NAME.firewall`
   (one TCP port, digits only). The root path unit `baft-tunnel-control-firewall@NAME.path` runs
   exactly `baft-tunnel firewall-read-helper --instance NAME` (oneshot, 15 s limit).
2. **Fixed, read-only commands only:** `/usr/sbin/nft list ruleset`, else `/usr/sbin/iptables-save`.
   No other command, no shell, no arguments from the agent beyond the validated port. Output is
   capped at 4 MiB. The helper holds `CAP_NET_ADMIN` in its own capability bound (kernel requirement
   to list); the agent and the tunnel do not.
3. **Reduced result, no rule text.** The helper writes `/run/baft-tunnel/firewall/NAME.json`
   (root-owned 0640, group `baft-tunnel-agent`; the directory is root 0750, so the agent cannot
   forge it): backend, `verdict`, counts, and address-set **names and element counts only**
   (never addresses) plus a `sets_digest`. `mutated` is always `false`.
4. **Scope:** IPv4 path only (nft families `ip` and `inet`, iptables IPv4), TCP, the INPUT hook, for
   the listening (serve) side of an instance registered with the agent.
5. **Verdicts → Preflight `firewall`:** `open` (no restricting input rule) and `allowed` (policy drop
   and an unscoped accept rule for the port) are **pass**; `no_rule` (policy drop, no accept) and
   `blocked_rule` (explicit drop/reject of the port) are **blocked**; `complex` (jumps such as ufw
   chains, port sets, source-scoped accepts, other drop rules whose order is not evaluated) and
   `unavailable` stay **Unknown**. Rule order is never evaluated, so only configurations where order
   cannot matter produce a decision. Without the helper installed or answering within 3 s the check
   stays Unknown (`pf.firewall.helper_missing`).
6. **Optional and explicit.** `install-agent-control.sh --instance NAME` enables the new path unit
   like the others; existing instances re-run it once. Nothing is ever opened, closed or changed;
   opening the port stays a manual operator step.
7. The connecting (forward) side stays Unknown: outbound filtering is out of scope.

## S10 (address-set drift)

This tier only reads. It is not a candidate cause of drift (no write path exists; the unit and its
tests contain no mutating verb). Its `sets_digest` (hash of table/set/element-count triples) lets a
LAB run record the digest before and after a stage so a change in set membership size is visible
and attributable to a time window. It cannot prove a clean firewall, and S10 stays open until the
source is found.

## Not claimed

No change of firewall state, no IPv6 verdict, no rule-order evaluation, no verdict for ufw/firewalld
chains that use jumps, no claim that a pass means the port is reachable from the peer (that is
`peer_reachability`).

## Evidence

Unit tests: nft and iptables-save samples for every verdict, IPv6-only rules ignored, request
validation and symlink refusal, forged-result directory refusal, no-helper path stays Unknown,
digest carries no addresses. Staged-install test asserts the unit, path unit, tmpfiles entry and the
absence of any mutating command. CI is not LAB: see the roadmap for the real-host acceptance run.
