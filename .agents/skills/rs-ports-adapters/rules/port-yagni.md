# port-yagni

> A port needs a second implementation or a test fake — otherwise don't add one
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Over-abstraction costs readability. The aim is replaceability where change is plausible, not an interface in front of everything.

## Add a port when…

| Signal | Example |
|---|---|
| a licence, price or hosting decision could replace it | databases, caches, brokers, search, cloud SDKs, LLM vendors |
| the business needs a test fake (slow, flaky, paid, or stateful dependency) | payments, email, clocks, downstream services |
| two implementations already exist or are planned | in-memory + real |
| an SDK is volatile or young (spec still moving) | MCP, A2A, newer cloud APIs |

## Don't when…

- It's `std`, `serde`, `tracing`, a pure utility, or a type that appears in your public API on purpose.
- The "port" would be a 1:1 mirror of one SDK's methods — that is a wrapper adding no decoupling; either shape it from the use case or call the SDK from an adapter directly at the edge.
- It's a one-off script.

## Cost control

Start with the port + memory fake + one real adapter. Add narrower traits when a second consumer needs less than the whole thing.

## See Also

- [port-design-by-need](port-design-by-need.md)
