# port-swap-drill

> Rehearse replacing a product on paper (and in a spike) before you need to
> **rs-ports-adapters · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Decoupling that was never exercised silently decays: a vendor type slips into a signature, a SQL string into a service. A swap drill finds the leaks while they are cheap.

## The drill (30 minutes, in review or design)

1. Pick one port and one hypothetical replacement ("what if we dropped ClickHouse / Redis / this SDK?").
2. List the files you would have to change. Expected: `adapters/<new>/` (new), `wiring.rs`, config schema, docs.
3. Anything else on the list is a leak: a vendor type, error, `cfg`, SQL string, or semantic assumption not in the port docs.
4. Fix the leak or write down why it stays (Design Contract *Avoid*/decision record).
5. For critical ports, build the **memory adapter first** — if it is hard to write, the port is too product-shaped.

## Review Signals

`python3 -I scripts/port-check.py . ` is the mechanical half; the drill is the judgement half. rs-review includes both.

## See Also

- [port-contract-tests](port-contract-tests.md)
- [port-no-vendor-in-domain](port-no-vendor-in-domain.md)
