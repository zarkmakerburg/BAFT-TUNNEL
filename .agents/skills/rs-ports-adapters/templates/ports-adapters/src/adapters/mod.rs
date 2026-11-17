//! # Design Contract
//! **Role:** Home of every backend implementation; one directory per product.
//! **Patterns:** Adapter per product (rs-ports-adapters)
//! **Invariants:** the only modules allowed to import vendor crates (`scripts/port-check.py` enforces it); each adapter passes the contract suite.
//! **Extend by:** new product → `adapters/<product>/mod.rs` implementing the ports it can serve, plus a `Backend` variant in `wiring`.
//! **Skills:** rs-ports-adapters

pub mod memory;
pub mod vendor_x;
