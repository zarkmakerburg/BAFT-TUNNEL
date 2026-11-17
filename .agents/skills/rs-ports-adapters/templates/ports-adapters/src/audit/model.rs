//! # Design Contract
//! **Role:** Domain types of the audit feature, expressed in business language only.
//! **Patterns:** Newtype ids (rs-type-driven) · Parse, don't validate
//! **Invariants:** no vendor derives (`Row`, `FromRow`, `Serialize` for a wire format) on these types; adapters map to their own row types.
//! **Extend by:** add a field here, then update every adapter's mapping — the contract suite fails until they agree.
//! **Skills:** rs-ports-adapters, rs-type-driven

/// Who performed the action.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActorId(pub String);

/// One audit fact. Time is epoch milliseconds in UTC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub at_ms: i64,
    pub actor: ActorId,
    pub action: String,
}

/// Half-open interval `[from_ms, to_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub from_ms: i64,
    pub to_ms: i64,
}

/// Number of events on one UTC day (`day` = days since the epoch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayCount {
    pub day: i64,
    pub count: u64,
}
