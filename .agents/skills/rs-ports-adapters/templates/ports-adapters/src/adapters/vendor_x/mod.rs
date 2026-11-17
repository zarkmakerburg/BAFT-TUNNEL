//! # Design Contract
//! **Role:** Backend for a columnar analytics store ("vendor X", e.g. ClickHouse): row mapping and error classification around the vendor client.
//! **Patterns:** Adapter · Anti-corruption layer (row type ≠ domain type) · Error classification
//! **Invariants:** vendor types (`Client`, `Row`, vendor errors) never leave this directory; every vendor error becomes a `StoreError` with a `StoreErrorKind`.
//! **Extend by:** add methods by mirroring the port; change the table schema only together with `AuditRow` and a migration.
//! **Skills:** rs-ports-adapters, rs-domain-error, rs-reuse-first

mod audit;
mod client;

pub use audit::VendorXAuditStore;
pub use client::{VendorClient, VendorError};
