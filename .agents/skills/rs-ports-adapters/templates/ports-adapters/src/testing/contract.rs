//! # Design Contract
//! **Role:** The cases every `AuditStore` must pass.
//! **Patterns:** Contract tests
//! **Invariants:** takes a factory so each case starts from an empty store; no vendor-specific assertions.
//! **Extend by:** add a case function and call it from `audit_store_contract`.
//! **Skills:** rs-testing

use crate::audit::{ActorId, AuditEvent, AuditStore, DayCount, TimeRange};

const DAY_MS: i64 = 86_400_000;

fn event(at_ms: i64) -> AuditEvent {
    AuditEvent { at_ms, actor: ActorId("alice".into()), action: "login".into() }
}

/// Run every contract case against stores produced by `make` (one fresh store per case).
///
/// # Panics
/// On the first violated expectation — this is test support.
pub async fn audit_store_contract<S: AuditStore>(make: impl Fn() -> S) {
    counts_group_by_utc_day_ascending(&make()).await;
    range_is_half_open(&make()).await;
    empty_range_yields_no_rows(&make()).await;
}

async fn counts_group_by_utc_day_ascending<S: AuditStore>(store: &S) {
    store.append(vec![event(2 * DAY_MS + 5), event(DAY_MS + 1), event(DAY_MS + 2)]).await.unwrap();
    let got = store.daily_counts(TimeRange { from_ms: 0, to_ms: 3 * DAY_MS }).await.unwrap();
    assert_eq!(got, vec![DayCount { day: 1, count: 2 }, DayCount { day: 2, count: 1 }]);
}

async fn range_is_half_open<S: AuditStore>(store: &S) {
    store.append(vec![event(DAY_MS), event(2 * DAY_MS)]).await.unwrap();
    let got = store.daily_counts(TimeRange { from_ms: DAY_MS, to_ms: 2 * DAY_MS }).await.unwrap();
    assert_eq!(got, vec![DayCount { day: 1, count: 1 }], "`to` is exclusive, `from` inclusive");
}

async fn empty_range_yields_no_rows<S: AuditStore>(store: &S) {
    store.append(vec![event(10)]).await.unwrap();
    let got = store.daily_counts(TimeRange { from_ms: 100, to_ms: 100 }).await.unwrap();
    assert_eq!(got, Vec::<DayCount>::new());
}
