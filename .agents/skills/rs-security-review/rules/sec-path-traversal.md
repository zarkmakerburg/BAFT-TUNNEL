# sec-path-traversal

> Join untrusted path components safely: Path::join with an absolute path replaces the base
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`Path::new("/srv/uploads").join("/etc/passwd")` is `/etc/passwd`: joining an absolute path **discards** the base. `..` components, symlinks and Windows prefixes (`C:`, `\\?\`) escape too. Archive extraction ("zip slip"), file download/upload APIs and config includes are the usual victims.

## Gates

A path (or component) comes from outside and is used with `Path::join`/`push`/`PathBuf::from`/`format!("{base}/{x}")` to open, create, delete or serve a file.

## Good

```rust
pub fn safe_join(base: &Path, untrusted: &str) -> io::Result<PathBuf> {
    let rel = Path::new(untrusted);
    // absolute paths would REPLACE the base; `..`/root/prefix components escape it
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path escapes base"));
    }
    let joined = base.join(rel);
    // symlinks: resolve what exists and re-check containment
    let base_canon = base.canonicalize()?;
    let mut probe = joined.clone();
    while !probe.exists() { if !probe.pop() { break; } }
    if !probe.canonicalize()?.starts_with(&base_canon) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "symlink escapes base"));
    }
    Ok(joined)
}
```
- Reject, don't "sanitise" by stripping `../` (`....//` tricks). Compare **components**, not strings.
- Strongest option: a capability-based API — `cap-std` (directory handles, no escaping) or `openat2` with `RESOLVE_BENEATH` on Linux.
- Store uploads under server-generated names (UUID), keep the user's name as metadata.
- Archives: apply the same check to every entry name; reject symlink entries that point outside.
- Canonicalisation alone has a time-of-check gap: combine with [sec-toctou-filesystem](sec-toctou-filesystem.md).

## False positives

Path constants or values from trusted config; component is validated to a single `Normal` segment from an allow-list.

## See Also

- [sec-toctou-filesystem](sec-toctou-filesystem.md)
