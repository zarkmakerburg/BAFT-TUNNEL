//! # Design Contract
//! **Role:** `AuditStore` backed by a mutex-guarded `Vec`.
//! **Patterns:** Adapter · Fake
//! **Invariants:** lock is never held across an `.await`; `daily_counts` sorts ascending and omits empty days.
//! **Extend by:** mirror any new `AuditStore` method here before touching real backends.
//! **Skills:** rs-ports-adapters, rs-concurrency

// The port is async; this adapter does no I/O, so its bodies have no `.await`.
#![allow(unknown_lints, clippy::unused_async_trait_impl)] // lint exists on newer clippy only

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::audit::{AuditEvent, AuditStore, DayCount, StoreError, StoreErrorKind, TimeRange};

const DAY_MS: i64 = 86_400_000;

#[derive(Default)]
pub struct MemoryAuditStore {
    events: Mutex<Vec<AuditEvent>>,
}

impl AuditStore for MemoryAuditStore {
    async fn append(&self, events: Vec<AuditEvent>) -> Result<(), StoreError> {
        let mut guard = self
            .events
            .lock()
            .map_err(|_| StoreError::new(StoreErrorKind::Internal, "memory store lock poisoned"))?;
        guard.extend(events);
        Ok(())
    }

    async fn daily_counts(&self, range: TimeRange) -> Result<Vec<DayCount>, StoreError> {
        let guard = self
            .events
            .lock()
            .map_err(|_| StoreError::new(StoreErrorKind::Internal, "memory store lock poisoned"))?;
        let mut by_day: BTreeMap<i64, u64> = BTreeMap::new();
        for e in guard.iter().filter(|e| e.at_ms >= range.from_ms && e.at_ms < range.to_ms) {
            *by_day.entry(e.at_ms.div_euclid(DAY_MS)).or_default() += 1;
        }
        Ok(by_day.into_iter().map(|(day, count)| DayCount { day, count }).collect())
    }
}
