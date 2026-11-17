# sec-lossy-conversions

> Lossy casts and lossy string conversions turn two different inputs into the same value
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`as` truncates and re-signs silently; `String::from_utf8_lossy`, `Path::to_string_lossy` and `OsStr::to_string_lossy` replace invalid bytes with U+FFFD, so **different** byte strings become **equal**. When the converted value is a username, path, key, tenant or policy subject, that is an authorisation or isolation bug.

## Gates

- `as` between integer types (`u64 as usize`, `i64 as u32`, `usize as i32`, negative → unsigned) on values that can exceed the target range.
- lossy string/OS-string conversion followed by comparison, lookup, authorisation or file access.
- `From`/`Into` impls that drop information (saturate, truncate, default on error).

## Do

```rust
let n = u32::try_from(len)?;                     // error instead of silent truncation
let i = usize::try_from(offset).map_err(|_| Error::OutOfRange)?;
let name = std::str::from_utf8(bytes)?;          // reject invalid UTF-8 for identifiers
let p: &str = path.to_str().ok_or(Error::NonUtf8Path)?;     // not to_string_lossy for decisions
```
- `as` only for deliberate, commented truncation/reinterpretation (`as u8` of a masked value); enable clippy `cast_possible_truncation`, `cast_sign_loss`, `cast_possible_wrap` (pedantic).
- Conversion traits: implement `TryFrom` when conversion can fail; `From` must be lossless.
- Display/logging may use lossy conversion; **security decisions must not**.
- Normalise (Unicode NFC, case-folding) at one boundary if identifiers need comparing, and compare the normalised form everywhere.

## See Also

- [sec-panic-on-input](sec-panic-on-input.md)
