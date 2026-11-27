# TP-U: fail-closed and denial-of-service behaviour of the UDP carrier

**Status:** Delivered 2026-11-27 for the prototype, behind the non-default `udp-carrier` feature.
This is gate (f) of [ADR-0046](ADR-0046-udp-datagram-carrier-design.md) — the sixth Acceptance
item, which follows gates (a), (c) and (d). (An earlier #244 comment said the item was unlettered;
the ADR's decision 9 does letter it (f), and this document uses that letter.) Gates (b) independent security review and (e) two-host LAB are still
open; nothing here is a product DoS guarantee or a Preview claim, and no user-facing surface selects
this carrier.

## 1. What this document is, and why this scope

ADR-0046 decision 4 makes five properties design constraints: no response larger than the triggering
datagram before address validation, a retry token (or equivalent), bounded per-peer state and
handshake concurrency, a listener that answers only well-formed QUIC and drops everything else
without exhaustible per-source allocation, and a fuzz target. The Acceptance list adds "documented
fail-closed and DoS behaviour" without saying what "documented" means.

The smallest defensible reading, adopted here:

1. a written analysis of the prototype's failure modes — what happens to the carrier and to the
   caller on handshake failure, datagram loss, peer restart, session close, oversized/hostile input
   and flooding;
2. a test for every behavioural claim a test can pin **in the prototype as merged**;
3. an explicit list of what is **not** established, so no reader mistakes this for an audit, a
   benchmark or a LAB result.

It is not a new gate, not a product-level DoS guarantee (no product path selects the carrier), and
not a substitute for gate (b). Nothing in this document was inferred from a design intention: every
claim below is marked as **tested**, **read from code**, or **unverified**.

Scope: `src/udp_carrier.rs` as merged in #477 and #478. The carrier has no product entry point: no
config key, CLI verb, API route or panel control, so in a default build no UDP socket, listener or
code path exists at all.

## 2. Failure modes

| Trigger | Observed behaviour | Direction | Evidence |
|---|---|---|---|
| Peer certificate not trusted / handshake cannot verify | No connection object is produced; the attempt errors or remains unresolved when the 2 s bound expires. Nothing can be sent on the data plane. | **Fails closed** | `udp_carrier::tests::handshake_failure_is_fail_closed` (tested) |
| Datagram lost in flight | It is not retransmitted and no duplicate sequence is delivered; the application sees loss, never a replayed frame. | Fails **open by design** (unreliable datagrams, ADR-0046 decision 5); no hidden recovery | `udp_carrier::tests::datagram_loss_is_not_retransmitted` (tested) |
| Session closed by the peer | Reads return an error instead of waiting; a caller cannot mistake a dead carrier for a slow one. | **Fails closed** | `udp_carrier::tests::closed_session_fails_reads_instead_of_hanging` (tested) |
| Peer process restart | The old connection ends as above. There is no reconnect, resumption or silent re-handshake in the module: a new session requires a new `connect`/`accept` and a new hello. Automatic reconnection is **not implemented**. | Fails closed (the caller decides) | read from code; no resumption code exists (read) |
| Oversized application payload | `encode_datagram` refuses a frame above 1200 bytes and `send_datagram` additionally refuses a frame above the peer's negotiated datagram size; nothing is put on the wire and the session stays usable. | **Fails closed per message** | `…::malformed_and_oversize_datagrams_fail_closed_per_message` (tested); `encode_datagram` (read) |
| Malformed frame from the peer (bad magic, version, declared length, trailing bytes) | `decode_datagram` refuses it; the caller's receive returns an error. A test peer that ignores that error keeps the **same session** working for later valid frames. | **Fails closed per message**, session survives | same test (tested); `decode_datagram` (read) |
| Malformed or oversized hello | `decode_hello` refuses bad magic, version, peer-id length, id charset and trailing bytes; `read_to_end(MAX_HELLO)` bounds the read at 128 bytes before parsing. | **Fails closed** | `datagram_and_hello_round_trip_and_refuse_malformed_input` (tested) |
| Unauthenticated or malformed QUIC datagram reaching the listener | It never reaches application code: QUIC only delivers after a completed handshake, and `accept` yields only completed connections. | **Fails closed** | read from code and from the quinn API contract (read; not a test of quinn itself) |
| Flood of well-formed datagrams on an established session | There is **no application-level rate limit**. Each datagram costs a bounded parse and a copy of at most 1185 payload bytes, and the caller's loop is unbounded. Transport-level flow control and the 256 KiB datagram buffers are quinn's, not ours. | **Open/unmitigated at the application layer** | read from code (`recv_datagram`, `transport_config`) |
| Flood of new handshakes | The prototype has no server loop at all — `accept` handles one connection per call. Any future server would have to bound concurrency, and no such bound exists in this module. | **Not implemented** | read from code |
| Memory growth per connection | Send and receive datagram buffers are each 256 KiB, idle timeout 20 s; per-message copies are bounded by the 1200-byte frame limit. | Bounded per connection, **unbounded per endpoint** (no connection cap) | `transport_config`, `decode_datagram` (read) |

## 3. Denial-of-service analysis, claim by claim

**Amplification.** ADR-0046 requires that no response be larger than the triggering datagram before
address validation. In this prototype that property is delegated entirely to `quinn`/`quinn-proto`:
the module sets no `use_retry`, no token policy and no custom anti-amplification logic, so whatever
protection exists is the dependency's default. **Unverified here.** This is the single most
important item for gate (b) to confirm, and it is why no UDP listener is exposed to users.

**Spoofed-source floods.** No retry token is configured, so source-address validation is whatever
quinn does by default. **Unverified.**

**Handshake concurrency.** No cap is set by this module; `accept` is a single-connection helper with
no accept loop, no semaphore and no per-IP accounting. A production listener would need an explicit
bound. **Not implemented**, deliberately: designing it now would invent the product surface ADR-0046
defers to Preview.

**Parser work per byte.** Every bounded frame is parsed in one pass with no allocation beyond the
payload copy; a malformed frame costs a few comparisons and one error. A fuzz target
(`fuzz/fuzz_targets/udp_datagram.rs`) exercises both parsers, and the CI `udp-datagram` job
smoke-runs it. That is evidence of robustness, not proof of no pathological input.

**Buffers and queueing.** `datagram_receive_buffer_size` and `datagram_send_buffer_size` are 256 KiB
each; when the receive buffer is full, quinn drops datagrams rather than growing memory. That
drop-instead-of-grow behaviour is the dependency's, **unverified by a test here**, and is the
correct failure direction for a datagram carrier.

**What a hostile peer cannot do (tested or read).** It cannot skip authentication (no plaintext
mode, and `accept` only yields completed handshakes), cannot make the module fragment or send above
1200 bytes, cannot make a malformed frame kill an established session through the documented API,
and cannot make the module retransmit a lost datagram.

**What a hostile peer can do (honest list).** On an established session it can send datagrams as
fast as congestion control and the receive buffer allow; the module has no rate limit, so the
application loop pays for each one. It can also hold a connection open until the 20 s idle timeout.
Both are prototype-scope limitations, acceptable only because no product listener exists.

## 4. Fail-closed vs fail-open, summarised

- **Closed:** authentication/verification, hello and frame validation, oversized sends, reads after
  session close. No fallback carrier exists anywhere in the module, so a UDP failure cannot silently
  become TCP (ADR-0046 decision 6 is structural here, not a policy check).
- **Open by design:** datagram delivery. Loss is visible to the caller and never repaired behind its
  back.
- **Open and unmitigated:** application-level flood handling, connection-count bounding and
  source-address validation. These are the gaps gate (b) must assess before any listener exists.

## 5. What this document does not prove

- It does not prove quinn's anti-amplification, retry behaviour or handshake resource limits are
  safe; none of that is configured or tested here.
- It is not a penetration test, a security review (gate (b)) or a LAB result (gate (e)); it says
  nothing about real paths, filtering, NAT rebinding, loss/jitter or throughput.
- It does not measure the cost of a flood, and it sets no performance or resilience threshold.
- It does not establish that a future product listener can be bounded; that design is deferred by
  ADR-0046 decision 8.
- It does not change product behaviour: `transport.udp` stays `Unknown`, `datagram` stays
  `Unsupported`, and the default build still contains zero `quinn` packages.

## 6. Reproduction

```console
cargo test --locked --features udp-carrier udp_carrier::tests::
cargo test --locked -- --list | grep -c udp_carrier        # 0 without the feature
cargo tree --locked -e normal --prefix none | grep -c '^quinn v'   # 0 in the default graph
```
