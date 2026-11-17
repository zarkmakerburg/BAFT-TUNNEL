//! # Design Contract
//! **Role:** Public API of the `audit` feature: re-exports the model, the port and the service.
//! **Patterns:** Feature module · Port in the feature, adapters elsewhere (rs-ports-adapters)
//! **Invariants:** nothing in this directory imports a database, cache or SDK crate.
//! **Extend by:** new capability → new method on the port (with a contract test) or a new narrow port; new rule → `service.rs`.
//! **Skills:** rs-ports-adapters, rs-structure

mod model;
mod ports;
mod service;

pub use model::{ActorId, AuditEvent, DayCount, TimeRange};
pub use ports::{AuditStore, StoreError, StoreErrorKind};
pub use service::AuditService;
