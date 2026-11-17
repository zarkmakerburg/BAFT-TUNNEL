//! # Design Contract
//! **Role:** Contract test suite shared by every `AuditStore` implementation.
//! **Patterns:** Contract tests (rs-ports-adapters, rs-testing) · Generic test over the port
//! **Invariants:** each assertion is a statement in the port's documentation; adapters never carry their own copy of these cases.
//! **Extend by:** add a case here whenever the port gains a method or a documented edge case; every adapter's test calls `audit_store_contract`.
//! **Skills:** rs-ports-adapters, rs-testing

mod contract;

pub use contract::audit_store_contract;
