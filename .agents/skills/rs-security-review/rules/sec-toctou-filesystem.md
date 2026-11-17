# sec-toctou-filesystem

> Check-then-use on the filesystem is a race: open first, then inspect the handle
> **rs-security-review · rule** — source: rs-skills original (2026-11-11)

## Why It Matters

`if path.exists() { File::open(path) }`, `metadata()` followed by an action, or "check ownership then write" lets another process swap the file (symlink, rename) between the check and the use. In privileged or shared-directory code (temp dirs, uploads, `/tmp`) that is a vulnerability, not just a flaky bug.

## Gates

A path is *checked* (`exists`, `is_file`, `metadata`, permissions, canonicalise) and later *used* by path, and an attacker with local write access to the directory can change it in between.

## Do

| Instead of | Use |
|---|---|
| `if !p.exists() { File::create(p) }` | `OpenOptions::new().write(true).create_new(true).open(p)` — atomic exclusive create (error if it exists) |
| `exists()` then `open` | `open` and handle `NotFound`; inspect the **opened handle** (`file.metadata()`), not the path |
| symlink following | `OpenOptionsExt::custom_flags(libc::O_NOFOLLOW)` (Unix); open the directory first and use `*at` calls / `cap-std` |
| predictable temp files | `tempfile` crate (`NamedTempFile`, `tempdir`) — random names, exclusive creation, cleanup |
| write-then-rename for atomic updates | write to a temp file in the **same directory**, `sync_all`, then `rename` |
| permission checks | create files with the right mode at creation (`OpenOptionsExt::mode`), don't `chmod` afterwards |

## False positives

Single-user private directories with no other writers; best-effort UX checks whose result is not security-relevant (handle the error at use anyway).

## See Also

- [sec-path-traversal](sec-path-traversal.md)
