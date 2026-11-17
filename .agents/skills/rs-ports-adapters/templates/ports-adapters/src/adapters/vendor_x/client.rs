//! # Design Contract
//! **Role:** Stand-in for the real vendor SDK so this template compiles; replace with the SDK's client (`clickhouse::Client`, `redis::aio::ConnectionManager`, …).
//! **Patterns:** Adapter seam
//! **Invariants:** in a real crate this file does not exist — `use <vendor_sdk>::Client` appears in `audit.rs` and nowhere outside `adapters/`.
//! **Extend by:** n/a (stand-in)
//! **Skills:** rs-ports-adapters, rs-reuse-first

// Stand-in client: a real SDK call awaits network I/O.
#![allow(unknown_lints, clippy::unused_async, clippy::unused_async_trait_impl)] // stand-in: a real SDK call awaits network I/O

use std::fmt;

/// Stand-in vendor error. Real SDKs expose their own enum; classify it in `audit.rs`.
#[derive(Debug)]
pub enum VendorError {
    Network(String),
    Timeout,
    BadRequest(String),
    Other(String),
}

impl fmt::Display for VendorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for VendorError {}

/// Stand-in client: the shape of calls a real SDK offers (insert rows, run a query).
#[derive(Default, Clone)]
pub struct VendorClient;

impl VendorClient {
    /// # Errors
    /// Always fails in this stand-in.
        pub async fn insert(&self, _table: &str, _rows: &[(i64, String, String)]) -> Result<(), VendorError> {
        Err(VendorError::Network("stand-in client: connect a real SDK".into()))
    }
}
