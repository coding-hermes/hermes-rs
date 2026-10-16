//! HRS-010 session-store vertical slice: the pinned Hermes session-store
//! operations (specs/persistence-shim.md §6) behind the HRS-002 persistence
//! boundary (§3), on the stock SQLite backend (§4).
//!
//! Layering (§3.1): callers depend on the engine-neutral [`api`] types only;
//! the concrete SQLite backend appears exclusively in
//! [`stock_sqlite::StockSqliteBackend::open`]. No alternative engine is
//! linked; HRS-009 owns any such evaluation.

pub mod api;
pub mod stock_sqlite;
#[cfg(feature = "turso")]
pub mod turso_backend;
pub mod upstream_ddl;

pub use api::{
    AppendMessage, BatchAppendReport, BusyKind, Capabilities, Capability, CapabilitySupport,
    CorruptScope, EndSessionOutcome, FtsCapabilities, JournalCapability, LeaseKind, MessageRow,
    ModelUsageDelta, OpenOptions, OpenReport, OpenStore, Patience, PersistenceError, ReadTxn,
    SchemaNeed, SearchHit, Session, SessionStore, StoreError, WriteTxn, RESET_END_REASONS,
    SCHEMA_VERSION_KNOWN,
};
pub use stock_sqlite::StockSqliteBackend;
#[cfg(feature = "turso")]
pub use turso_backend::TursoBackend;
