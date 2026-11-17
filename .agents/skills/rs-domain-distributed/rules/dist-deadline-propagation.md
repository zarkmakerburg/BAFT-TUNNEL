# dist-deadline-propagation

> Every call has a deadline, and downstream calls inherit what is left
> **rs-domain-distributed · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Without deadlines a slow dependency holds resources forever; without propagation a downstream call keeps working for a caller that already gave up.

## Do

```rust
let remaining = deadline.saturating_duration_since(Instant::now());
let reply = tokio::time::timeout(remaining, client.call(req)).await.map_err(|_| Error::DeadlineExceeded)??;
// forward: gRPC timeout metadata / X-Request-Deadline header with the REMAINING time
```
- Set a default at the edge; never "no timeout" in config.
- Cancel work when the deadline passes (drop the future; make the work cancel-safe — rs-async-patterns).
- Map to a classified error (`Timeout`) so retry policy applies (rs-domain-error).

## See Also

- [dist-retry-budget](dist-retry-budget.md)
