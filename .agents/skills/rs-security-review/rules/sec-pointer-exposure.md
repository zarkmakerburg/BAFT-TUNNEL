# sec-pointer-exposure

> Don't print addresses: pointers in logs, errors or responses defeat ASLR
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

Heap and stack addresses (`{:p}`, `Debug` of raw pointers/references, `Box::into_raw` values in messages, hashing a pointer as an id) tell an attacker where code and data live, turning a memory-corruption bug that needed a guess into a reliable exploit. Alone it is low severity; next to any `unsafe` or FFI bug it is an amplifier.

## Gates

`{:p}`, `{:?}` on `*const T`/`*mut T`/`fn` pointers, `as usize`/`addr()` of a pointer, or pointer-derived ids reach a log, error message, API response, metrics label or file that a less-privileged party can read.

## Do

- Log stable ids (request id, object id), never addresses.
- Use counters/UUIDs for object identity instead of `ptr as usize`.
- Error types shown to callers carry no addresses; keep `Debug` for internal logs and review derives on structs holding raw pointers.
- In FFI wrappers, hide handle internals behind opaque ids.

## False positives

Debug-only builds; addresses printed only to a developer-local terminal (still avoid committing such logging).

## See Also

- [rs-observability obs-no-sensitive-data](../../rs-observability/rules/obs-no-sensitive-data.md)
