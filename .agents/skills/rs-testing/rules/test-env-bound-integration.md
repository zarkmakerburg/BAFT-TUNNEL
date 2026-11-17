# test-env-bound-integration

> Integration tests that need external tooling compile everywhere, run where the tooling exists — and the scaffold test is replaced, not kept
> **rs-testing · rule** — source: rs-skills, learned on the MCP Flex gateway policies (2026-11)

## Why It Matters

Gateway, cloud and embedded projects often have integration harnesses that need Docker, a
vendor CLI, credentials or hardware. Left as-is they fail `cargo test` everywhere else, so
people stop running tests; deleted, nothing covers the wiring. The PDK project template
ships a `hello` test with a fake config that no real policy accepts — it can never pass.

## Do

1. Unit-test every contract Invariant in the crate (pure functions, fakes for host traits).
2. Replace the template test with real scenarios through a small shared harness
   (`tests/common::start(config)`), one per externally visible guarantee
   (reject without credentials, block an injection, propagate a header…).
3. Mark them `#[ignore = "needs make build + Docker; run with make test"]` and make the
   project's test entry point run them (`cargo test -- --include-ignored`).
4. CI still **compiles and lints** them (`cargo test --no-run`, `clippy --all-targets`).
5. Report honestly: "compiled, not executed here" is a Low finding until they run somewhere.

## See Also

- [test-integration-dir](test-integration-dir.md)
- rs-domain-gateway — the Flex PDK harness
