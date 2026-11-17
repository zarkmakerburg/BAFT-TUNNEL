//! # Design Contract
//! **Role:** `AuditStore` on top of the vendor client.
//! **Patterns:** Adapter · Row type separate from the domain type · Error classification table
//! **Invariants:** `AuditRow` is the only type that knows the table layout; `classify` is the only place that reads `VendorError`.
//! **Extend by:** new port method → new query here + a row mapping; new vendor error → one more arm in `classify`.
//! **Skills:** rs-ports-adapters, rs-domain-error

// `daily_counts` is a stub in this template; a real adapter awaits the vendor query.
#![allow(unknown_lints, clippy::unused_async_trait_impl)] // lint exists on newer clippy only

use crate::audit::{AuditEvent, AuditStore, DayCount, StoreError, StoreErrorKind, TimeRange};

use super::client::{VendorClient, VendorError};

/// Wire/row shape. A real SDK adds its derives here (e.g. `#[derive(clickhouse::Row, serde::Serialize)]`), never on `AuditEvent`.
struct AuditRow {
    at_ms: i64,
    actor: String,
    action: String,
}

impl From<AuditEvent> for AuditRow {
    fn from(e: AuditEvent) -> Self {
        Self { at_ms: e.at_ms, actor: e.actor.0, action: e.action }
    }
}

/// The one place vendor errors are interpreted.
fn classify(e: VendorError) -> StoreError {
    let kind = match &e {
        VendorError::Network(_) => StoreErrorKind::Unavailable,
        VendorError::Timeout => StoreErrorKind::Timeout,
        VendorError::BadRequest(_) => StoreErrorKind::Rejected,
        VendorError::Other(_) => StoreErrorKind::Internal,
    };
    StoreError::new(kind, "vendor call failed").with_source(e)
}

pub struct VendorXAuditStore {
    client: VendorClient,
    table: String,
}

impl VendorXAuditStore {
    #[must_use]
    pub fn new(client: VendorClient, table: impl Into<String>) -> Self {
        Self { client, table: table.into() }
    }
}

impl AuditStore for VendorXAuditStore {
    async fn append(&self, events: Vec<AuditEvent>) -> Result<(), StoreError> {
        let rows: Vec<(i64, String, String)> = events
            .into_iter()
            .map(AuditRow::from)
            .map(|r| (r.at_ms, r.actor, r.action))
            .collect();
        self.client.insert(&self.table, &rows).await.map_err(classify)
    }

    async fn daily_counts(&self, _range: TimeRange) -> Result<Vec<DayCount>, StoreError> {
        // A real adapter runs the vendor query here, e.g. a GROUP BY toDate(at) …, and maps rows to `DayCount`.
        Err(StoreError::new(StoreErrorKind::Internal, "stand-in: implement with the real SDK"))
    }
}
