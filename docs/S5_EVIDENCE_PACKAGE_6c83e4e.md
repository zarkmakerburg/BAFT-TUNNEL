# S5 evidence package — `6c83e4e`

**Status: S5 remains OPEN.** This package is an input to an independent review. It is not a
security approval, it does not close S5, and no human sign-off has been recorded anywhere in it.
The closure decision belongs to a reviewer outside the authors and implementers of the reviewed
changes (see §1 of [SECURITY_AUDIT_WHOLE_MAIN_2026-11-24.md](SECURITY_AUDIT_WHOLE_MAIN_2026-11-24.md)).

| | |
|---|---|
| Prepared by | Codebuff agent (contributed to the code under review) — **not independent** |
| Prepared on | 2026-11-24 |
| Evidence level | Source review at the exact SHA + hosted CI record; no local build or test run |
| S5 status after this package | **OPEN** |

> **2026-11-25 follow-up:** S5-F1, S5-F2, S5-F3 and the `Host` cardinality item are fixed on
> `main` at `ffbc367` (PR #439), with regression tests for the exact-boundary head cap, the
> ASCII-digit `Content-Length` rule and the one-`Host` rule. That changes the code, not the
> review: this package still describes the `6c83e4e` artifact, and S5 remains OPEN until a
> reviewer outside the implementers signs off on the final SHA.

## 1. Artifact under review

| Item | Value |
|---|---|
| Repository | `zarkmakerburg/BAFT-TUNNEL` |
| Commit | `6c83e4ed62be58a7b114108a28042688005765cc` |
| Parent | `82ac04da718aab7a2f7cf97659d35913bb2b1196` (docs-only S5 audit, PR #432) |
| Branch | `main` (HEAD at the time of preparation); no tag contains this commit |
| Subject | `fix(agent): refuse ambiguous body framing and require one valid Host (S5-01) (#433)` |
| Author / committer | `Goldapp <zarkmakerburg@gmail.com>` / GitHub (`web-flow`) |
| GitHub commit verification | `verified: true` (GitHub-signed merge commit) |
| Files changed | `src/agent.rs` only: +139 / −22 |
| Hosted CI on this SHA | 10 of 10 check runs completed `success` (see <https://github.com/zarkmakerburg/BAFT-TUNNEL/commit/6c83e4e/checks>). The legacy commit-status endpoint is empty; only check runs were counted |

Local clone check used for every citation below:

```console
git clone https://github.com/zarkmakerburg/baft-tunnel
cd baft-tunnel && git rev-parse HEAD
# 6c83e4ed62be58a7b114108a28042688005765cc
```

## 2. Scope

**In scope.**

- S5-01 as filed: ambiguous HTTP message framing in the agent, and the `Host` requirement, at
  `6c83e4e`.
- The post-fix parser and router in `src/agent.rs`, and the tests added in the same commit.
- Screening question from the requester: does `src/fleet_hub.rs` at the same SHA contain the same
  framing gap? (Answer and findings in §5. This is a screening, not a full audit of the fleet
  listener.)

**Out of scope.**

- Re-reviewing S5-02..S5-05 (unchanged Info findings).
- Any file not named above: `src/fleet.rs`, `src/fleet_run.rs`, `src/fleet_report.rs`, TLS
  material handling, systemd units, `panel/`, workflows.
- Dynamic exploitation, fuzzing, LAB runs, dependency audit, release or deploy.
- Any closure statement. This package deliberately contains none.

## 3. What changed in `6c83e4e` (agent surface)

All line numbers are at `6c83e4e`.

1. **`ReadOutcome` replaces the `Option`/`usize::MAX` sentinel** — `src/agent.rs:508-517`.
   `Closed`, `HeadTooLarge`, `BadRequest(&'static str)` are now distinct, so a 400 reason reaches
   the client instead of being collapsed into "no request".
2. **`read_request` refuses ambiguous framing** — `src/agent.rs:520-572`:
   - any `Transfer-Encoding` → 400 (`:537-541`);
   - more than one `Content-Length` → 400 (`:542-546`);
   - a `Content-Length` that is empty or not ASCII digits → 400 (`:546-556`). The comment records
     why: `usize::from_str` accepts a leading sign, and HTTP defines a length as digits only.
3. **One valid `Host` is required before routing or authentication** — `src/agent.rs:431-441`
   (`host_name`), `:485-493` (400 for missing/duplicate/whitespace/control Host), `:445-458`
   (`allowed_host`, now refuses a missing Host instead of allowing it).
4. **`header_values` returns every occurrence and trims only SP/HTAB** — `src/agent.rs:773-785`.
   The comment states that a bare CR or LF must stay visible so the Host and Content-Length checks
   reject it instead of trimming it into a valid value.
5. **Body slicing** is `&buf[head_end..head_end + length]` (`src/agent.rs:497`); the declared
   length is the only thing routed to `route`.
6. **Tests added** — `src/agent.rs:4689-4725`:
   - `host_is_required_and_unambiguous` (`:4690`): missing, empty, duplicated, space-containing and
     bare-LF Host values are 400; one `127.0.0.1:18080` Host is 200.
   - `ambiguous_body_framing_is_refused` (`:4706`): `Transfer-Encoding`, duplicate
     `Content-Length`, comma-joined `0, 1`, `-1`, `1x`, `+0`, `+1` and a bare-LF length are 400;
     one well-formed `Content-Length: 0` still routes (405 for the GET-only health route).

## 4. Verification actually performed, and its limits

| Check | Result |
|---|---|
| Clone and `git rev-parse HEAD` equals the reviewed SHA | PASS (local) |
| `git show --stat` limited to `src/agent.rs` | PASS (local) |
| Read the post-fix parser, Host check, body slice and the new tests | PASS (source read) |
| GitHub check runs on the exact SHA | 10/10 `success` (hosted CI, read through the API) |
| Build the crate locally | **NOT RUN** — no `cargo` or `rustc` in this workspace |
| Run any test locally | **NOT RUN** — same reason; the new tests were read, not executed |
| Dynamic request-smuggling attempt against a running agent | **NOT RUN** |
| LAB / real host / deploy / release / tag | **NOT RUN** — none attempted |

CI green on the exact SHA is evidence that the repository's suite passed there; it is not a
substitute for an independent reviewer executing the negative cases, and it says nothing about
`fleet_hub.rs`, which has no test for the cases in §5.

## 5. Screening result: `src/fleet_hub.rs` at `6c83e4e`

**Answer: it does not contain the full S5-01 gap, but it contains a residual of the same class.**
It already refuses transfer codings and duplicate lengths; it still normalises the length value in
ways HTTP/1.1 does not allow, so a proxy and this listener can disagree about the body boundary.

### 5.1 Already correct (no finding)

`read_request` (`src/fleet_hub.rs:155-218`) does the following:

- rejects any `Transfer-Encoding`, `Expect` or `Content-Encoding` (`:197`);
- rejects a second `Content-Length` (`:191-194`);
- rejects a missing `Content-Length` with 411 (`:201`);
- rejects a body longer than the declared length (`:205-207`): bytes **already buffered** after
  the declared body refuse the request (400). Bytes that arrive later are never read, because the
  listener serves one request per connection and closes it after the answer, so they cannot reach
  a second request on that connection;
- requires exactly `POST /v1/fleet/report HTTP/1.1` before reading a body (`:174-188`);
- caps the report body (`:202-204`) and runs behind mandatory mTLS 1.3 with a Fleet-CA chain and
  CRL (`serve`, `:229-261`); the listener is default-off and decides no policy.

Because the listener is mTLS-only, has one fixed route, and does not route on `Host`, the
DNS-rebinding half of S5-01 has no equivalent surface here. The framing half does.

### S5-F1 — `Content-Length` value is trimmed and parsed leniently — **Low**

**Evidence [CODE].** `src/fleet_hub.rs:190-196`:

```rust
match name.trim().to_ascii_lowercase().as_str() {
    "content-length" => {
        if length.is_some() { return Err(400); }
        length = Some(value.trim().parse::<usize>().map_err(|_| 400u16)?);
    }
```

`str::trim` removes all Unicode whitespace, including a bare `CR` and `LF`, and
`usize::from_str` accepts a leading `+`. Therefore all of these are accepted as a length:

- `Content-Length: 5\n` (bare LF immediately before the terminating `\r\n\r\n`);
- `Content-Length: +5`;
- `Content-Length:\t5` (HTAB is legal OWS, so this one is benign).

The first two are exactly the cases `6c83e4e` added to the agent parser, with the agent's own
comment at `src/agent.rs:547-549` and its tests at `:4706-4719` recording the reason. The fleet
listener kept the pre-fix pattern.

**Impact.** A front proxy that rejects a bare LF or a signed length, while this listener accepts
it, produces a framing disagreement of the S5-01 kind. Exploitation requires the ability to reach
the listener with a Fleet client certificate (the TLS layer authenticates first), so this is not a
pre-auth surface, and no deployed proxy in front of the fleet listener is documented in this
repository. It is a robustness/correctness gap, not a demonstrated bypass.

**Suggested fix (not applied here).** Share the agent's rule: accept the value only if it is
non-empty and every byte is an ASCII digit, and trim only `' '`/`'\t'`; add a test next to
`http_layer_maps_rules_and_bounds_requests` (`src/fleet_hub.rs:375-470`) for `+5`, `-1`, `5\n` and
` 5 `.

### S5-F2 — Whitespace between header name and colon is accepted — **Info**

**Evidence [CODE].** `src/fleet_hub.rs:189-190` splits on the first `:` and normalises the name
with `name.trim().to_ascii_lowercase()`. A line `Content-Length : 5` is therefore treated as a
framing header. RFC 9112 §5.1 forbids whitespace between field-name and colon; recipients must
reject such a message or treat the line as invalid, and different intermediaries do different
things with it.

The fixed agent parser does not trim the name (`src/agent.rs:773-785` compares the raw key), so the
two components disagree on this input as well — in the opposite direction: the agent ignores it and
reads a zero-length body, while `fleet_hub` honours it.

**Impact.** Informational without an intermediary that accepts the form. It is listed because it
is the same "two components can disagree on the body boundary" property, and because a future
reviewer should decide one rule for both parsers.

**Suggested fix (not applied here).** Refuse a header line whose name contains SP/HTAB, and add
one regression case. Matching the agent's exact-name comparison is not a substitute: it would
silently ignore `Content-Length : 5` instead of rejecting the message, which is a different way
for the two parsers to disagree about the same input.

### S5-F3 — Head cap can be overshot by one read — **Info**

**Evidence [CODE].** `src/fleet_hub.rs:161-172` checks `buf.len() > MAX_HEAD_BYTES` before the
`read`, so a head of up to `MAX_HEAD_BYTES + 1024` bytes can still be accepted if the delimiter
arrives in the same chunk. Bounded and small; not a framing issue.

**Suggested fix.** Optional: check the cap after extending the buffer as well, as the agent does
with `>=`.

### 5.2 Explicit non-findings / differences from the agent

- No `Host` validation. Not treated as a finding: no browser surface, one fixed route, mTLS before
  any HTTP byte, and no `Host`-based routing. If an L7 proxy is ever placed in front, revisit this
  together with S5-F1.
- The listener still has `#![allow(dead_code)]` and is default-off; that was not changed here.

## 6. Questions for the independent reviewer

Please answer each explicitly, with the evidence class you used (source, test run, CI, dynamic).

1. Does `6c83e4e` close S5-01 on the agent surface as filed, for all four classes: transfer
   coding, duplicate `Content-Length`, malformed `Content-Length`, and ambiguous/missing `Host`?
2. Is there any input where `read_request` (`src/agent.rs:520-572`) accepts a request whose body
   boundary differs from what an RFC 9112 intermediary would compute? Consider bare CR, bare LF,
   `+`/`-` signs, comma-joined values, obs-fold, and a header line without a colon.
3. `header_values` trims only SP/HTAB; confirm that a bare CR or LF cannot reach `route` inside
   either `Host` or `Content-Length` and be normalised into a valid value.
4. `allowed_host` now returns `false` for a request without `Host` (`src/agent.rs:445-458`).
   Is rejecting HTTP/1.0-style requests acceptable for this loopback-only API, and does any
   supported client or test still rely on the old permissive behaviour?
5. The body is taken as `&buf[head_end..head_end + length]` (`:497`) and there is no keep-alive
   loop. Can bytes after the declared body be smuggled into a later request on the same
   connection, or are they discarded with the connection?
6. Is the `431`/`413` ordering at `:467-493` correct for a request that is both oversized and
   has no valid `Host`, and does that ordering matter to any front proxy?
7. Do the added tests (`:4689-4725`) actually exercise the wire path (real socket + `handle`),
   or only a helper? Do they cover every 400 reason added by the commit?
8. For `src/fleet_hub.rs`, do you agree that S5-F1 and S5-F2 are the S5-01 class, and at what
   severity in the fleet listener's actual deployment (mTLS client required, default off)?
9. Should `fleet_hub`'s `Content-Length` parser adopt the agent's ASCII-digit rule in the same
   change as S5-01, or is a separate follow-up acceptable? Either way S5 stays OPEN until the
   independent review says otherwise.
10. Does closure of S5 require dynamic execution of the negative cases on a real listener, given
    that this package could not build or run the suite?

## 7. Reviewer sign-off (intentionally blank)

No approval has been recorded. Only a human reviewer outside the authors and implementers of
`6c83e4e` may fill this in; an agent must not.

```
Reviewer name:
Affiliation / independence from the authors and implementers:
Date:
Scope reviewed:
Result:            [ ] S5-01 closed   [ ] S5-01 remains open   [ ] new finding(s)
Signature / link to signed statement:
```

## 8. Reproduction commands

```console
git clone https://github.com/zarkmakerburg/baft-tunnel
cd baft-tunnel
git checkout 6c83e4ed62be58a7b114108a28042688005765cc
git show --stat 6c83e4e
sed -n '425,500p' src/agent.rs      # Host requirement
sed -n '520,590p' src/agent.rs      # framing refusals
sed -n '4689,4760p' src/agent.rs    # added tests
sed -n '155,220p' src/fleet_hub.rs  # fleet screening target
```

With a Rust toolchain on Linux, the agent tests are:

```console
cargo test --bin baft-tunnel host_is_required_and_unambiguous
cargo test --bin baft-tunnel ambiguous_body_framing_is_refused
```

and the fleet regression tests for §5 now exist, added after this package at `ffbc367`:
`head_cap_is_enforced_at_the_boundary_without_overshoot`,
`content_length_must_be_plain_digits_without_header_whitespace` and
`host_must_appear_exactly_once_without_control_bytes` in `src/fleet_hub.rs`.

## 9. Limits

- Everything in §5 is source-level. No request was sent to a running listener.
- No build, no local test run, no fuzz, no LAB, no host access, no deploy.
- `src/fleet_hub.rs` was screened for framing only; its certificate identity rules, TLS verifier
  and the `fleet.rs` rules were not re-audited here.
- The CI result is read from GitHub for the exact SHA; it is not a reproduction of the run.
- This package records no approval and changes no status. S5 remains **OPEN**.
