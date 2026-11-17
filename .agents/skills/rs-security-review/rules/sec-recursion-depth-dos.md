# sec-recursion-depth-dos

> Bound recursion over attacker-shaped data: deserialization, formatting, traversal, Drop
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Stack exhaustion is not a panic: the process **aborts** (SIGSEGV on the guard page), `catch_unwind` cannot help, and one request can kill the whole server. Attackers need only deep nesting (`[[[[…]]]]`, a 100 000-node linked list) — a few KB of input. Threads have small stacks (main thread ~8 MiB on Linux; spawned and tokio worker threads default to ~2 MiB) and each recursive frame costs hundreds of bytes to several KB.

## Gates (flag only when all hold)

1. The code recurses (directly or through `Display`/`Debug`/`Serialize`/`Deserialize`/`Drop`) over a structure whose depth an outsider controls (request body, file, message, AST, tree, linked list).
2. No depth cap applies at this site. Text codecs such as `serde_json` cap nesting by default (check the docs of your codec and whether the cap was disabled or raised); binary codecs such as `bincode` and `postcard` leave depth to the caller; **hand-written** `Deserialize`/visitors/parsers and your own tree walkers have no cap unless you wrote one.
3. The input reaches the code from outside the trust boundary.

## Bad

```rust
fn count(v: &serde_json::Value) -> usize {            // recursion depth = input depth
    match v { Value::Array(a) => 1 + a.iter().map(count).sum::<usize>(), _ => 1 }
}
// also: dropping a 1_000_000-element `Option<Box<Node>>` list recurses in Drop
```

## Good

```rust
pub fn depth(v: &serde_json::Value, max: usize) -> Result<usize, &'static str> {
    fn go(v: &serde_json::Value, d: usize, max: usize) -> Result<usize, &'static str> {
        if d > max { return Err("nesting too deep"); }
        match v {
            Value::Array(a)  => a.iter().try_fold(d, |m, x| go(x, d + 1, max).map(|k| m.max(k))),
            Value::Object(o) => o.values().try_fold(d, |m, x| go(x, d + 1, max).map(|k| m.max(k))),
            _ => Ok(d),
        }
    }
    go(v, 0, max)
}
```
- Reject over-deep input **before** the recursive walk (length-prefix/scan pass, or thread a depth counter through `DeserializeSeed`/`Visitor`).
- Iterate with an explicit stack/queue instead of recursing; implement `Drop` for long linked structures iteratively (`while let Some(next) = cur.take()`).
- A body-size limit alone does not bound depth: `[[[[…]]]]` is dense.
- `serde_stacker`-style wrappers grow the stack on the heap when a cap is not acceptable.

## False positives (don't file)

Cap in effect (default `serde_json` limit, an explicit depth check upstream); input statically trusted (`include_str!`, config shipped with the binary); structure depth bounded by type (`[T; N]`).

## See Also

- [sec-resource-exhaustion](sec-resource-exhaustion.md)
- [sec-panic-on-input](sec-panic-on-input.md)
