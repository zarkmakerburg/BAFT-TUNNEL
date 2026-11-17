//! # Design Contract
//! **Role:** Composition root for the audit feature: turns configuration into a concrete backend.
//! **Patterns:** Composition root · Enum dispatch (zero-cost runtime selection, no `dyn`, no `async_trait`)
//! **Invariants:** the only non-adapter file that names backends; business code receives `AnyAuditStore` or any `S: AuditStore`.
//! **Extend by:** new backend → one `Backend` variant, one arm in `build` and one in each `AuditStore` method of `AnyAuditStore`.
//! **Skills:** rs-ports-adapters

use crate::adapters::memory::MemoryAuditStore;
use crate::adapters::vendor_x::{VendorClient, VendorXAuditStore};
use crate::audit::{AuditEvent, AuditStore, DayCount, StoreError, TimeRange};

/// Selected by configuration (`AUDIT_BACKEND=memory|vendor_x`) — switching products is a config change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    Memory,
    VendorX { table: String },
}

pub enum AnyAuditStore {
    Memory(MemoryAuditStore),
    VendorX(VendorXAuditStore),
}

#[must_use]
pub fn build(backend: &Backend) -> AnyAuditStore {
    match backend {
        Backend::Memory => AnyAuditStore::Memory(MemoryAuditStore::default()),
        Backend::VendorX { table } => AnyAuditStore::VendorX(VendorXAuditStore::new(VendorClient, table.clone())),
    }
}

impl AuditStore for AnyAuditStore {
    async fn append(&self, events: Vec<AuditEvent>) -> Result<(), StoreError> {
        match self {
            Self::Memory(s) => s.append(events).await,
            Self::VendorX(s) => s.append(events).await,
        }
    }

    async fn daily_counts(&self, range: TimeRange) -> Result<Vec<DayCount>, StoreError> {
        match self {
            Self::Memory(s) => s.daily_counts(range).await,
            Self::VendorX(s) => s.daily_counts(range).await,
        }
    }
}
