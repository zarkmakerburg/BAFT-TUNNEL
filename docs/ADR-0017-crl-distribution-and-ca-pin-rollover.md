# ADR-0017: remote CRL distribution and CA/pin rollover

- **Status:** ACCEPTED (owner approved 2026-11-07; recommended A1+B1). Nothing here is implemented yet.
- **Date:** 2026-11-07
- **Scope:** Phase 1 two-node deployment, offline CA (ADR-0012, ADR-0015). Closes the open gates "remote CRL distribution" and "CA/pin rollover" as design only.

## Context

ADR-0015 made local CRL activation safe but leaves two gaps: (1) the CRL is copied between hosts by hand, and a CRL that expires (`next_update`) makes every TLS handshake fail closed; (2) there is no procedure for replacing the CA certificate or a node certificate pin without downtime or without trusting both old and new material forever.

Constraints: the CA key stays offline; nodes must not gain signing authority; no component may silently fetch trust material from an unauthenticated source; the Iran-side host may have unreliable access to the outside.

## Part A: CRL distribution — options

| Option | How | Pros | Cons |
|---|---|---|---|
| **A1 (recommended)** Operator push over SSH, verified node-side | Operator runs `scripts/pki` helper that `scp`s the signed CRL to each node and invokes the ADR-0015 activation helper per node | No new network surface or daemon; reuses audited activation and rollback; works with offline CA | Manual; operator must act before `next_update` |
| A2 Pull from HTTPS endpoint | Node timer fetches CRL from a URL | Automatic | New outbound dependency, censorship exposure, availability/downgrade risk, fetch must be pinned to the CA anyway |
| A3 Peer-to-peer relay over the tunnel | Peer serves its CRL on a control stream | Uses existing mTLS | Couples revocation to the channel being revoked; a revoked peer could serve a stale CRL |

**Recommendation: A1**, plus guardrails: issue CRLs with a validity of at least 14 days and re-issue weekly; the runtime already warns when under 24 hours remain (`warn_if_crl_expiring`) and exposes `seconds_until_expiry` in the support bundle; add an operator checklist item and an optional systemd timer that only *reports* expiry (never fetches). Defer A2/A3 until there are more than a handful of nodes.

## Part B: CA and pin rollover — options

| Option | How | Pros | Cons |
|---|---|---|---|
| **B1 (recommended)** Overlap window with two trust anchors | Add the new CA certificate to the trust bundle (old+new), issue new node certs from the new CA, update pins in a fixed order, then remove the old CA after an explicit deadline | Zero-downtime; each step reversible; no flag day | Requires runtime support for multiple CAs and multiple accepted peer pins; both CRLs must be loaded during the overlap |
| B2 Flag-day replacement | Stop both ends, replace CA, certs, pins, restart | Simplest code | Guaranteed outage; partial rollout leaves the tunnel down |
| B3 Cross-signing | New CA cross-signed by old CA | Standard PKI pattern | Extra chain complexity; rustls path handling and CRL for the intermediate path need new code and tests |

**Recommendation: B1.** Required changes (not yet built):
1. Accept a bundle of CA certificates and a CRL per CA; revocation is checked against the CRL of the issuing CA.
2. Accept up to two pins per peer (`peer_fingerprints`) during rollover; reject more than two.
3. Rollover sequence: (a) issue new CA offline; (b) on both nodes add new CA + its CRL (activation helper, one node at a time); (c) issue new node certs, install, add new pins; (d) verify handshakes with the new certs on both directions; (e) remove old pins, then old CA, after a stated deadline. Each step has a rollback to the previous state.
4. Compromise path: if the old CA key is suspected compromised, skip the overlap — flag-day (B2) with an out-of-band pin check is the only safe option.
5. Tests: unit tests for dual-CA/dual-pin acceptance and rejection; CI test for the full sequence; LAB run on the isolated instance before any claim.

## Consequences

- With A1+B1 the CA never goes online and nodes never fetch trust material.
- B1 adds new trust-parsing code and so needs the same adversarial testing as the existing TLS path; until implemented, CA/pin rollover remains an **open gate** and flag-day replacement is the only supported procedure.
- Independent security review of B1 is recommended before release beyond pre-release; it was waived for the current pre-release, not performed.

## Decision requested

1. Approve A1 for CRL distribution? (default: yes)
2. Approve B1 for CA/pin rollover and the implementation work above? (default: yes)
