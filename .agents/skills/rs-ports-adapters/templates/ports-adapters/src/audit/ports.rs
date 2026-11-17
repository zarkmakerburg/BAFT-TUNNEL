//! # Design Contract
//! **Role:** The `AuditStore` port: what the audit feature needs from storage, in its own words.
//! **Patterns:** Port / Repository (rs-ports-adapters) · Narrow capability trait · Error classification
//! **Invariants:** methods express intent (`daily_counts`), never a query language; errors are classified, never vendor types.
//! **Extend by:** add a method only when a use case needs it, then extend `testing::audit_store_contract` in the same change.
//! **Avoid:** `fn query(&self, sql: &str)`; returning vendor row types; a generic `Repository<T>` CRUD trait.
//! **Skills:** rs-ports-adapters, rs-error-handling, rs-domain-error

use std::future::Future;

use super::model::{AuditEvent, DayCount, TimeRange};

/// Why a store call failed — what callers can act on, independent of the product behind the port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErrorKind {
    /// Temporary: retry with backoff (network, overload, failover).
    Unavailable,
    /// The call took too long; the result is unknown.
    Timeout,
    /// The input cannot be stored as given (caller bug or schema mismatch).
    Rejected,
    /// Anything else; not safe to retry blindly.
    Internal,
}

/// Port error: a kind plus the vendor error kept as `source` for logs, never matched on.
#[derive(Debug, thiserror::Error)]
#[error("audit store {kind:?}: {message}")]
pub struct StoreError {
    pub kind: StoreErrorKind,
    pub message: String,
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl StoreError {
    #[must_use]
    pub fn new(kind: StoreErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), source: None }
    }

    #[must_use]
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Retry policy lives with the caller (rs-domain-error); this only says whether it makes sense.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(self.kind, StoreErrorKind::Unavailable | StoreErrorKind::Timeout)
    }
}

/// Storage the audit feature needs. `Send` futures so callers can spawn.
pub trait AuditStore: Send + Sync {
    /// Append events; either all are stored or the call fails (callers may retry the whole batch).
    fn append(&self, events: Vec<AuditEvent>) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Events per UTC day inside `range`, ascending by day, days without events omitted.
    fn daily_counts(&self, range: TimeRange) -> impl Future<Output = Result<Vec<DayCount>, StoreError>> + Send;
}
