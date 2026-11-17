//! # Design Contract
//! **Role:** Library root of the ports-and-adapters example: wires the `audit` feature to its replaceable backends.
//! **Patterns:** Ports & adapters (rs-ports-adapters) · Composition root in `wiring` · Enum dispatch for runtime backend choice
//! **Invariants:** `audit::` never imports a vendor crate; only `adapters::*` and `wiring` name backends.
//! **Extend by:** a new backend = a new directory under `adapters/` + one `Backend` variant in `wiring`; it must pass `testing::audit_store_contract`.
//! **Layout:** `audit/` feature (model, ports, service) · `adapters/<product>/` · `testing/` contract suite · `wiring.rs`
//! **Skills:** rs-ports-adapters, rs-structure, rs-testing

pub mod adapters;
pub mod audit;
pub mod testing;
pub mod wiring;
