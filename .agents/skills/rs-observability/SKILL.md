---
name: rs-observability
description: "Use when adding logging, tracing, metrics or diagnostics to Rust code, or when debugging production behaviour. Triggers: observability, logging, log, tracing, span, #[instrument], structured logging, log levels, RUST_LOG, EnvFilter, tracing-subscriber, OpenTelemetry, OTLP, metrics, Prometheus, correlation id, request id, redact, sensitive data in logs, error chain in logs, what should I log, too much logging, debug in production"
user-invocable: false
---

# Observability

> **Layer 4: Delivery** · when it misbehaves in production, what will we see?

## Core Question

**When this misbehaves at 3 a.m., what will the on-call engineer see — and is it enough to act without redeploying?**

Observability is designed, not sprinkled. Decide the *spans* (units of work),
the *fields* (what identifies the work) and the *levels* (who should be woken)
before writing the first `info!`.

---

## Thinking Prompt

1. **What is the unit of work?** Request, job, message, batch — that is a span (`#[instrument]`). Everything inside inherits its fields.
2. **What identifies it?** `request_id`, `tenant`, `order_id` — structured fields, not formatted strings.
3. **What would I grep for?** Then don't grep: emit it as a field and let the backend filter.
4. **What must never appear?** Secrets, tokens, PII — redact at the type level (`Debug` impl or newtype) so it cannot leak.
5. **Who is woken by this level?** `error` = someone acts; `warn` = someone looks tomorrow; `info` = business event; `debug`/`trace` = off in prod.

---

## The rules

| Concern | Do | Rule |
|---|---|---|
| Library vs application | libraries emit `tracing` events only; applications install the subscriber | `obs-tracing-over-log`, `obs-library-facade` |
| Units of work | `#[tracing::instrument(skip(big_args), fields(order_id = %id))]` on boundaries | `obs-instrument-spans` |
| Fields, not strings | `info!(order_id = %id, amount = ?amt, "order placed")` | `obs-structured-fields` |
| Levels and filtering | `EnvFilter` from `RUST_LOG`; `info` default; `debug` per module | `obs-levels-filter` |
| Errors | log the chain once at the boundary (`error = ?e` or `%e` with sources), not at every `?` | `obs-error-chain` |
| Sensitive data | newtypes with redacting `Debug`; never log raw headers/bodies | `obs-no-sensitive-data` |

## Where the instrumentation goes (rs-structure)

```
api/handlers        span per request: method, route, request_id, tenant      ← info events: accepted/rejected
domain/service      span per use case: order_id, step                        ← info: business events, warn: policy violations
infra/adapters      span per external call: system, operation, duration      ← error: failures with chain
main / app.rs       subscriber setup, OTLP exporter, log format (json in prod)
```
The Design Contract's **Role** line usually names the span.

## Production checklist

- [ ] one span per request/job with a correlation id propagated across services
- [ ] JSON output in production, pretty in dev (`tracing-subscriber` `fmt` layer)
- [ ] every `error!` includes the error chain and enough fields to find the record
- [ ] no secrets or PII in any field — enforced by types, checked in review
- [ ] metrics for the four golden signals (latency, traffic, errors, saturation) if the service has an SLO (rs-domain-cloud-native)
- [ ] log volume at `info` is bounded per request (no per-item `info!` in loops)

## Anti-Patterns

| Anti-pattern | Why bad | Better |
|---|---|---|
| `println!`/`eprintln!` in a library | caller can't filter or route | `tracing` events |
| `log::info!("user {} did {}", a, b)` | unqueryable | structured fields |
| logging at every `?` | one failure → ten log lines | log once at the boundary with the chain |
| `debug!` with a full request body | PII leak, volume | redacted summary fields |
| `#[instrument]` on a hot inner loop | overhead, noise | instrument the boundary, `trace!` inside if needed |

## Trace Up ↑ / Trace Down ↓

| Need | See |
|---|---|
| what counts as a business event | rs-domain-model, rs-domain-* |
| error chains worth logging | rs-error-handling, rs-domain-error |
| spans across `await` and tasks | rs-concurrency |
| exporter/config wiring and release settings | rs-release, rs-domain-cloud-native |

<!-- rules-index:start -->
## Rules (8)

Depth rules under `rules/` — open the one the question needs, not all of them.

**obs-**

- [`obs-error-chain`](rules/obs-error-chain.md) — Log errors with their full source chain, and log each error exactly once
- [`obs-instrument-spans`](rules/obs-instrument-spans.md) — Use `#[tracing::instrument]` and spans to attach context to async tasks and requests
- [`obs-levels-filter`](rules/obs-levels-filter.md) — Use log levels meaningfully and filter with `EnvFilter` / `RUST_LOG`
- [`obs-library-facade`](rules/obs-library-facade.md) — Libraries emit through the tracing/log facade and never install a subscriber
- [`obs-no-reflect-untrusted`](rules/obs-no-reflect-untrusted.md) — Never put the content you just blocked or masked into the reply, a header, or an INFO log
- [`obs-no-sensitive-data`](rules/obs-no-sensitive-data.md) — Never log secrets or PII; redact or skip them
- [`obs-structured-fields`](rules/obs-structured-fields.md) — Record structured key-value fields, not values interpolated into the message string
- [`obs-tracing-over-log`](rules/obs-tracing-over-log.md) — Use `tracing` for structured, span-aware diagnostics instead of `println!` or bare `log`

<!-- rules-index:end -->

---

## Related Skills

| When | See |
|---|---|
| Measuring instead of logging | rs-performance |
| Testing that spans/fields exist | rs-testing |
