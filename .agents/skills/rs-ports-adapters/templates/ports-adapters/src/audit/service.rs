//! # Design Contract
//! **Role:** Audit business rules; depends on the `AuditStore` port only.
//! **Patterns:** Application service generic over the port (static dispatch) · Dependency inversion
//! **Invariants:** never names a backend; never matches on vendor errors; empty batches are not sent to the store.
//! **Extend by:** new use case = new method here calling port methods; new storage need = extend the port first.
//! **Skills:** rs-ports-adapters, rs-domain-error

use super::model::{AuditEvent, DayCount, TimeRange};
use super::ports::{AuditStore, StoreError};

pub struct AuditService<S: AuditStore> {
    store: S,
}

impl<S: AuditStore> AuditService<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Record events; an empty batch is a no-op.
    ///
    /// # Errors
    /// The store's [`StoreError`]; check [`StoreError::is_transient`] before retrying.
    pub async fn record(&self, events: Vec<AuditEvent>) -> Result<(), StoreError> {
        if events.is_empty() {
            return Ok(());
        }
        self.store.append(events).await
    }

    /// Events per day inside `range`.
    ///
    /// # Errors
    /// The store's [`StoreError`].
    pub async fn activity(&self, range: TimeRange) -> Result<Vec<DayCount>, StoreError> {
        self.store.daily_counts(range).await
    }
}
