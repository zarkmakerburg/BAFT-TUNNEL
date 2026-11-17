//! # Design Contract
//! **Role:** In-memory backend: the fake for tests and the reference implementation of every port's semantics.
//! **Patterns:** Adapter · Fake (rs-testing)
//! **Invariants:** behaves exactly as the port documents (ordering, omitted empty days); not for production data volumes.
//! **Extend by:** implement each new port here first — it defines the expected behaviour for the other adapters.
//! **Skills:** rs-ports-adapters, rs-testing

mod audit;

pub use audit::MemoryAuditStore;
