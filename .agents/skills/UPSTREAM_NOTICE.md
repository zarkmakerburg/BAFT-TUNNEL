# Notices

`rs-skills` is released under the MIT licence (see `LICENSE`). Parts of it are
adapted from third-party works that carry their own licences. Those licences
require the notices below to stay with the copies; they apply only to the
material described and do not imply any endorsement.

## 1. Rust skills and rules adapted from MIT-licensed projects

The 265 rule files adapted from the second work below (marked by their ids, e.g.
`own-`, `err-`, `mem-`, `async-`, `perf-`, `api-`), the skills, agents, commands,
hooks and templates ported from the first (renamed `rs-*`), and the `checks/`
compile-verification tooling are adapted from the following MIT-licensed works.

- **actionbook/rust-skills** — 2.1.0, commit `5c40d3a` — MIT.
  Copyright (c) the actionbook/rust-skills authors.
- **leonardomso/rust-skills** — 1.5.1, commit `fd2a861` — MIT.
  Copyright (c) 2025 Leonardo Maldonado.

The MIT licence text, which applies to the adapted material from each of the two
works above:

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## 2. Rust Design Patterns (MPL-2.0)

The 50 pages under `skills/rs-patterns/` reproduce, with small modifications,
the "Rust Design Patterns" book by the rust-unofficial authors (commit
`36fdd17`, 2026-09-21), licensed under the **Mozilla Public Licence 2.0**.

- Each page keeps its MPL header block.
- `skills/rs-patterns/LICENSE-MPL` holds the licence text.
- `skills/rs-patterns/SOURCES.md` lists the modifications made to each file, as
  the MPL requires. The corresponding source is the upstream repository named there.

The MPL applies to those files only; everything else is MIT.
