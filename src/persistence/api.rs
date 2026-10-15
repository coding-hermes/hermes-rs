//! Engine-neutral persistence-shim API (specs/persistence-shim.md §3).
//!
//! Every type here is plain data + traits: no SQLite (or any engine) type
//! leaks through a caller-facing signature (§3.1 engine-leak rule). The
//! stock SQLite backend implements [`SessionStore`] and is named only in
//! its own module's constructor.

use std::fmt;
use std::time::Duration;

/// Write-patience budget for a write transaction (§3.3(2): upstream
/// defaults 20 s generic / 60 s transcript / 0.5 s activity).
#[derive(Debug, Clone, Copy)]
pub struct Patience(pub Duration);

impl Patience {
    pub const WRITE: Patience = Patience(Duration::from_secs(20));
    pub const TRANSCRIPT: Patience = Patience(Duration::from_secs(60));
    pub const ACTIVITY: Patience = Patience(Duration::from_millis(500));
}

/// The schema version this build knows (upstream SCHEMA_VERSION at the pin,
/// `hermes_state_common.py:283` — 31). Files written by a newer version are
/// refused, never silently downgraded (§3.6).
pub const SCHEMA_VERSION_KNOWN: i64 = 31;

/// Upstream end reasons that advance `conversation_generations`
/// (§2.4: `_RESET_END_REASONS`, `hermes_state_common.py:178` at the pin).
pub const RESET_END_REASONS: [&str; 6] = [
    "session_reset",
    "session_switch",
    "idle",
    "daily",
    "suspended",
    "resume_pending_expired",
];

/// Busy/lock contention class (§3.4: classified by result code first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyKind {
    /// SQLITE_BUSY: the database file is locked by another connection.
    Busy,
    /// SQLITE_LOCKED: an inner table/schema object is locked.
    Locked,
    /// Cancellation observed while waiting for the lock (§3.5).
    Abandoned,
}

/// Lease kinds surfaced as typed errors (§3.3(5)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseKind {
    CompressionLock,
    TurnLease,
}

/// Corruption scope (§3.4: `SQLITE_CORRUPT_VTAB` ⇒ FTS-scoped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorruptScope {
    Store,
    MalformedSchema,
    FtsIndex,
}

/// Schema-level blocker on open (§3.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaNeed {
    /// File written by a NEWER schema version than this build knows.
    Newer(i64),
    /// File needs a migration this build must run before serving reads.
    MigrationRequired { from: i64, to: i64 },
}

/// One error taxonomy, mapped 1:1 to the upstream cause tuple (§3.4).
#[derive(Debug)]
pub enum PersistenceError {
    Busy {
        kind: BusyKind,
    },
    LeaseLost {
        lease: LeaseKind,
    },
    Corrupt {
        scope: CorruptScope,
    },
    DiskFull,
    Replaced,
    WalGenerationLost,
    SessionRowMissing,
    FtsUnavailable {
        reason: String,
    },
    Schema {
        need: SchemaNeed,
    },
    Unsupported {
        capability: Capability,
    },
    /// §3.3(4): the callback started and settlement is unknown; the caller
    /// re-runs the idempotent operation after checking state.
    UnknownOutcome,
    Io(std::io::Error),
    /// Store-level invariant violation with context (message-store shape
    /// mismatches, malformed inputs). Not an upstream cause tuple member:
    /// slice-local input contract.
    InvalidInput(String),
    /// Transcript write refused: the session lineage is compression-closed
    /// (upstream CompressionSessionClosedError, §2.4 write guards).
    CompressionClosed,
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistenceError::Busy { kind } => write!(f, "store busy: {kind:?}"),
            PersistenceError::LeaseLost { lease } => write!(f, "lease lost: {lease:?}"),
            PersistenceError::Corrupt { scope } => write!(f, "corrupt store: {scope:?}"),
            PersistenceError::DiskFull => write!(f, "disk full"),
            PersistenceError::Replaced => write!(f, "database file replaced under the handle"),
            PersistenceError::WalGenerationLost => write!(f, "WAL generation lost"),
            PersistenceError::SessionRowMissing => write!(f, "session row missing"),
            PersistenceError::FtsUnavailable { reason } => write!(f, "FTS unavailable: {reason}"),
            PersistenceError::Schema { need } => write!(f, "schema: {need:?}"),
            PersistenceError::Unsupported { capability } => {
                write!(f, "capability ruled out: {capability:?}")
            }
            PersistenceError::UnknownOutcome => {
                write!(
                    f,
                    "transaction settlement unknown; re-check state before retry"
                )
            }
            PersistenceError::Io(e) => write!(f, "io error: {e}"),
            PersistenceError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            PersistenceError::CompressionClosed => {
                write!(f, "compression-closed session refuses transcript writes")
            }
        }
    }
}

impl std::error::Error for PersistenceError {}

/// Store-operation error (validation/lease/row-shape), distinct from
/// engine-level [`PersistenceError`].
#[derive(Debug)]
pub enum StoreError {
    LeaseLost {
        lease: LeaseKind,
    },
    SessionRowMissing,
    /// Appending into a compression-closed session (upstream
    /// CompressionSessionClosedError, §2.4 guards).
    CompressionClosed,
    InvalidInput(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::LeaseLost { lease } => write!(f, "lease lost: {lease:?}"),
            StoreError::SessionRowMissing => write!(f, "session row missing"),
            StoreError::CompressionClosed => {
                write!(f, "compression-closed session refuses transcript writes")
            }
            StoreError::InvalidInput(m) => write!(f, "invalid input: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

/// Capability kinds the caller can query (§3.2). Plain data; no engine names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    JournalWal,
    SearchFts,
    SearchTrigram,
    SearchCjk,
    CheckpointPassive,
    BulkParameters(u32),
}

/// Answer to [`Capabilities::supports`] — silent degradation is forbidden
/// (§3.2 rules): the answer carries the reason on every non-Yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilitySupport {
    Yes,
    No { reason: String },
    Degraded { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalCapability {
    Wal,
    RollbackOnly,
}

#[derive(Debug, Clone)]
pub struct FtsCapabilities {
    pub base: Result<(), String>,
    pub trigram: Result<(), String>,
    pub cjk: Result<(), String>,
}

#[derive(Debug, Clone)]
pub struct Capabilities {
    pub journal: JournalCapability,
    pub fts: FtsCapabilities,
    pub checkpoint_passive_only: bool,
    /// OFD-lock WAL-generation guard (§2.11); false on this slice's
    /// backend until the lockguard surface is ported (recorded, not silent).
    pub advisory_generation_guard: bool,
    pub file_format_standard_sqlite: bool,
    pub bulk_parameter_limit: u32,
}

impl Capabilities {
    pub fn supports(&self, c: Capability) -> CapabilitySupport {
        match c {
            Capability::JournalWal => match self.journal {
                JournalCapability::Wal => CapabilitySupport::Yes,
                JournalCapability::RollbackOnly => CapabilitySupport::No {
                    reason: "journal resolved to rollback (delete) mode".into(),
                },
            },
            Capability::SearchFts => match &self.fts.base {
                Ok(()) => CapabilitySupport::Yes,
                Err(reason) => CapabilitySupport::No {
                    reason: reason.clone(),
                },
            },
            Capability::SearchTrigram => match &self.fts.trigram {
                Ok(()) => CapabilitySupport::Yes,
                Err(reason) => CapabilitySupport::No {
                    reason: reason.clone(),
                },
            },
            Capability::SearchCjk => match &self.fts.cjk {
                Ok(()) => CapabilitySupport::Yes,
                Err(reason) => CapabilitySupport::Degraded {
                    reason: reason.clone(),
                },
            },
            Capability::CheckpointPassive => {
                if self.checkpoint_passive_only {
                    CapabilitySupport::Yes
                } else {
                    CapabilitySupport::No {
                        reason: "passive checkpoint unavailable".into(),
                    }
                }
            }
            Capability::BulkParameters(n) => {
                if n <= self.bulk_parameter_limit {
                    CapabilitySupport::Yes
                } else {
                    CapabilitySupport::No {
                        reason: format!(
                            "batch of {n} exceeds this backend's parameter limit {}",
                            self.bulk_parameter_limit
                        ),
                    }
                }
            }
        }
    }
}

impl CapabilitySupport {
    /// True only for a clean Yes (tests/convenience).
    pub fn yes(&self) -> bool {
        matches!(self, CapabilitySupport::Yes)
    }
}

/// Journal-mode preference at open (§3.2: upstream-equivalent knobs only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalPreference {
    /// Upstream default: WAL with verified read-back, DELETE fallback.
    Default,
    /// Explicit rollback-journal request.
    RollbackOnly,
    /// Never create; fail rather than scaffold.
    NoCreate,
}

#[derive(Debug, Clone)]
pub struct OpenOptions {
    pub path: std::path::PathBuf,
    pub journal: JournalPreference,
    pub create_if_missing: bool,
    pub read_only: bool,
}

impl OpenOptions {
    pub fn read_write(path: impl Into<std::path::PathBuf>) -> OpenOptions {
        OpenOptions {
            path: path.into(),
            journal: JournalPreference::Default,
            create_if_missing: true,
            read_only: false,
        }
    }
    pub fn read_only(path: impl Into<std::path::PathBuf>) -> OpenOptions {
        OpenOptions {
            path: path.into(),
            journal: JournalPreference::Default,
            create_if_missing: false,
            read_only: true,
        }
    }
}

/// What open() observed (§3.6 `OpenReport`).
#[derive(Debug, Clone, Default)]
pub struct OpenReport {
    pub schema_version_before: Option<i64>,
    pub schema_version_after: Option<i64>,
    pub fts_storage_version: Option<i64>,
    pub columns_reconciled: Vec<String>,
    pub migrations_applied: Vec<String>,
    pub degraded: Vec<String>,
    pub created_new_store: bool,
    /// Path an invalid-header file was quarantined to (§2.10), if any.
    pub quarantined: Option<String>,
}

/// The value returned by a successful open (§3.2 sketch).
#[derive(Debug)]
pub struct OpenStore {
    pub capabilities: Capabilities,
    pub report: OpenReport,
}

/// One message row as the store reads/writes it (upstream data conventions
/// preserved verbatim: REAL epoch seconds, TEXT ids, JSON-as-TEXT).
#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<String>,
    pub tool_name: Option<String>,
    pub timestamp: f64,
    pub token_count: Option<i64>,
    pub message_uid: Option<String>,
    pub active: bool,
    pub compacted: bool,
    pub display_kind: Option<String>,
}

/// Message append request (§6.1 `append_message`).
#[derive(Debug, Clone, Default)]
pub struct AppendMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<String>,
    pub tool_name: Option<String>,
    pub timestamp: Option<f64>,
    pub token_count: Option<i64>,
    /// Durable per-logical-message id; minted when absent (§2.8: 32
    /// lowercase hex — `uuid4().hex` upstream, `randomblob(16)` in the
    /// trigger; both are 32 lowercase hex chars).
    pub message_uid: Option<String>,
}

/// Per-model usage rollup delta (§6.1 `record_model_usage` upsert).
#[derive(Debug, Clone, Default)]
pub struct ModelUsageDelta {
    pub model: String,
    pub billing_provider: String,
    pub billing_base_url: String,
    pub billing_mode: String,
    pub task: String,
    pub api_call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub estimated_cost_usd: f64,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
}

/// Session row projection (§2.2 `sessions`; slice-relevant columns).
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub source: String,
    pub model: Option<String>,
    pub billing_provider: Option<String>,
    pub billing_base_url: Option<String>,
    pub billing_mode: Option<String>,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub api_call_count: i64,
    pub estimated_cost_usd: Option<f64>,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub end_reason: Option<String>,
    pub parent_session_id: Option<String>,
    pub session_key: Option<String>,
}

/// `end_session` outcome (§6.1: first end wins; generation bump only when
/// a row actually changed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndSessionOutcome {
    /// This call wrote the boundary.
    Ended { generation_bumped: bool },
    /// A previous end already won; this call was a no-op.
    AlreadyEnded,
}

/// Batch-append report (§6.1 `append_messages_batch`: one transaction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchAppendReport {
    pub first_id: i64,
    pub appended: usize,
}

/// One transaction's read access (§3.3 `ReadTxn`); engine-neutral handle.
pub trait ReadTxn {
    /// Messages in `messages.id` ascending order; `active`/`compacted`
    /// inclusion flags per §6.1 `get_messages`.
    fn get_messages(
        &mut self,
        session_id: &str,
        include_inactive: bool,
        include_compacted: bool,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<Vec<MessageRow>, StoreError>;

    /// The session row projection.
    fn get_session(&mut self, session_id: &str) -> Result<Option<Session>, StoreError>;

    /// FTS5 MATCH search (§6.1 `search`): BM25-rank order, visibility
    /// predicates per upstream `_search_filter_clauses`. Capability-gated
    /// by the caller (§3.7(3)).
    fn search_messages(
        &mut self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, StoreError>;
}

/// Search hit projection (§6.1): message id + session + snippet.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub message_id: i64,
    pub session_id: String,
    pub role: String,
    pub snippet: String,
    pub rank: f64,
}

/// Write-transaction handle: parameterized statements for the §6 operation
/// set; the store owns commit (§3.3).
pub trait WriteTxn {
    fn ensure_session(
        &mut self,
        session_id: &str,
        source: &str,
        model: Option<&str>,
    ) -> Result<(), StoreError>;
    fn append_message(&mut self, session_id: &str, msg: &AppendMessage) -> Result<i64, StoreError>;
    fn end_session(
        &mut self,
        session_id: &str,
        end_reason: &str,
    ) -> Result<EndSessionOutcome, StoreError>;
    fn record_model_usage(
        &mut self,
        session_id: &str,
        delta: &ModelUsageDelta,
    ) -> Result<(), StoreError>;
}

/// The caller-facing store trait (§3.3 sketch). `T` is carried through
/// opaquely; commit is owned by the store; `f` MUST be idempotent under
/// retry (it may be replayed whole after a busy wait).
pub trait SessionStore: Send + Sync {
    fn write<T>(
        &self,
        patience: Patience,
        f: &mut dyn FnMut(&mut dyn WriteTxn) -> Result<T, StoreError>,
    ) -> Result<T, PersistenceError>;

    fn read<T>(
        &self,
        f: &mut dyn FnMut(&mut dyn ReadTxn) -> Result<T, StoreError>,
    ) -> Result<T, PersistenceError>;

    fn capabilities(&self) -> &Capabilities;
    fn open_report(&self) -> &OpenReport;
    /// PASSIVE-only checkpoint policy (§2.7): runtime checkpoints never
    /// truncate; `CheckpointCapability` stays `{ PassiveOnly }`.
    fn checkpoint_passive(&self) -> Result<(), PersistenceError>;
    /// Backup-API snapshot to `dest` (§6.3(2), F-8 leg at slice scale).
    fn backup(&self, dest: &std::path::Path) -> Result<(), PersistenceError>;
    /// Flush + close; acknowledged writes must be observable after reopen.
    fn close(&mut self) -> Result<(), PersistenceError>;
}
