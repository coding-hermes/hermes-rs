//! Stock SQLite backend for the HRS-010 session-store slice
//! (specs/persistence-shim.md §4 + §6).
//!
//! File/schema compatibility rules implemented here:
//! - upstream DDL is embedded VERBATIM (upstream_ddl.rs, generated from the
//!   pin) and applied idempotently; no renamed/re-typed/re-ordered columns;
//! - additive reconciliation only, with upstream's own defaults;
//! - no engine-private schema objects; files stay standard SQLite;
//! - REAL epoch-second timestamps, TEXT ids, JSON-as-TEXT, AUTOINCREMENT
//!   rowids, `conversation_generations` never garbage-collected;
//! - journal resolution with verified read-back and DELETE fallback,
//!   PASSIVE-only runtime checkpoints, foreign keys ON;
//! - error classification code-first (busy/lock/corrupt-scope/disk-full).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::ffi::ErrorCode;
use rusqlite::{Connection, OpenFlags};

use super::api::{
    AppendMessage, BusyKind, Capabilities, Capability, CapabilitySupport, CorruptScope,
    EndSessionOutcome, FtsCapabilities, JournalCapability, JournalPreference, MessageRow,
    ModelUsageDelta, OpenOptions, OpenReport, Patience, PersistenceError, ReadTxn, SchemaNeed,
    SearchHit, Session, SessionStore, StoreError, WriteTxn, RESET_END_REASONS,
    SCHEMA_VERSION_KNOWN,
};
use super::upstream_ddl::{
    FRESH_STORE_CENSUS_JSON, UPSTREAM_DEFERRED_INDEX_SQL, UPSTREAM_FTS_SQL,
    UPSTREAM_FTS_TRIGRAM_SQL, UPSTREAM_SCHEMA_SQL,
};

/// Upstream `_SQL_IN_CHUNK` (§2.4): 900-parameter IN-list cap.
pub const SQL_IN_CHUNK: usize = 900;
/// Upstream `MAX_FTS5_QUERY_CHARS` (§2.6).
const MAX_FTS5_QUERY_CHARS: usize = 2_048;
/// Upstream `journal_size_limit` (§2.7): 64 MiB.
const JOURNAL_SIZE_LIMIT_BYTES: i64 = 64 * 1024 * 1024;
/// FTS storage-layout version stamped on fresh stores (§2.9, v23 layout = 3).
const FTS_STORAGE_VERSION: i64 = 3;

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn jitter_sleep() {
    // Small bounded jitter so concurrent writers do not sync-stepped on the
    // same retry schedule (§3.3(2): retry with jitter inside the patience).
    let ms = 5 + (std::process::id() % 17) as u64;
    std::thread::sleep(Duration::from_millis(ms));
}

/// Map a rusqlite error to the §3.4 taxonomy, code-first.
fn classify(err: &rusqlite::Error) -> PersistenceError {
    match err {
        rusqlite::Error::SqliteFailure(ffi, msg) => {
            let text = msg.clone().unwrap_or_default();
            let extended = ffi.extended_code;
            match ffi.code {
                ErrorCode::DatabaseBusy => PersistenceError::Busy {
                    kind: BusyKind::Busy,
                },
                ErrorCode::DatabaseLocked => PersistenceError::Busy {
                    kind: BusyKind::Locked,
                },
                ErrorCode::OperationInterrupted => PersistenceError::Busy {
                    kind: BusyKind::Abandoned,
                },
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => {
                    // §3.4: SQLITE_CORRUPT_VTAB (267) is FTS-scoped even with
                    // generic prose; bare corrupt/not-a-database is store-scoped.
                    if extended == 267 {
                        PersistenceError::Corrupt {
                            scope: CorruptScope::FtsIndex,
                        }
                    } else {
                        PersistenceError::Corrupt {
                            scope: CorruptScope::Store,
                        }
                    }
                }
                ErrorCode::SystemIoFailure if extended == 13 || text.contains("disk is full") => {
                    PersistenceError::DiskFull
                }
                ErrorCode::SystemIoFailure => PersistenceError::Io(std::io::Error::other(format!(
                    "sqlite io failure ({extended}): {text}"
                ))),
                ErrorCode::ReadOnly => PersistenceError::Io(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("read-only store: {text}"),
                )),
                _ => {
                    // Disk-full prose guard (§3.4: MUST NOT classify as corrupt
                    // even though "disk" appears in corruption prose).
                    if text.contains("database or disk is full") {
                        PersistenceError::DiskFull
                    } else if extended == 267 {
                        PersistenceError::Corrupt {
                            scope: CorruptScope::FtsIndex,
                        }
                    } else {
                        PersistenceError::Io(std::io::Error::other(format!(
                            "sqlite error ({extended}): {text}"
                        )))
                    }
                }
            }
        }
        other => PersistenceError::Io(std::io::Error::other(format!("store error: {other}"))),
    }
}

fn is_busy(err: &PersistenceError) -> bool {
    matches!(
        err,
        PersistenceError::Busy {
            kind: BusyKind::Busy | BusyKind::Locked
        }
    )
}

/// Pre-open header probe (§2.10/§4.2): a 0-byte or non-SQLite page 0 is
/// quarantined before a fresh scaffold, never silently overwritten.
fn has_invalid_sqlite_header_preopen(path: &Path) -> Result<bool, PersistenceError> {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return Ok(false); // absent: open() decides via create_if_missing
    };
    let mut header = [0u8; 16];
    let meta = f.metadata().map_err(PersistenceError::Io)?;
    if meta.len() == 0 {
        return Ok(true);
    }
    f.read_exact(&mut header)
        .map_err(|e| PersistenceError::Io(std::io::Error::new(e.kind(), "short header read")))?;
    Ok(&header != b"SQLite format 3\x00")
}

fn file_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path).ok()?;
    Some((m.dev(), m.ino()))
}

fn quarantine_invalid_state_db(path: &Path) -> Result<PathBuf, PersistenceError> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let target = path.with_extension(format!("db.quarantine-{ts}"));
    std::fs::rename(path, &target).map_err(PersistenceError::Io)?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            let target_sidecar = PathBuf::from(format!("{}{suffix}", target.display()));
            std::fs::rename(sidecar, target_sidecar).map_err(PersistenceError::Io)?;
        }
    }
    Ok(target)
}

fn fresh_census_matches(conn: &Connection) -> Result<bool, PersistenceError> {
    let expected = serde_json_value::Value::parse(FRESH_STORE_CENSUS_JSON)?;
    for typ in ["table", "index", "view", "trigger"] {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%'")
            .map_err(|e| classify(&e))?;
        let mut names: Vec<String> = stmt
            .query_map([typ], |r| r.get::<_, String>(0))
            .map_err(|e| classify(&e))?
            .filter_map(|r| r.ok())
            .collect();
        names.sort();
        let expected_names: Vec<String> = expected.get(typ).cloned().unwrap_or_default();
        if names != expected_names {
            return Ok(false);
        }
    }
    Ok(true)
}

// Minimal JSON-value reader for the embedded census (avoids a serde dep for
// one generated, self-controlled constant).
mod serde_json_value {
    use super::PersistenceError;

    #[derive(Debug, Clone)]
    pub struct Value {
        pub obj: Vec<(String, Vec<String>)>,
    }

    impl Value {
        pub fn parse(text: &str) -> Result<Value, PersistenceError> {
            fn expect<'a>(
                rest: &'a str,
                key: &str,
            ) -> Result<(&'a str, Vec<String>), PersistenceError> {
                let marker = format!("\"{key}\": [");
                let idx = rest.find(&marker).ok_or_else(|| {
                    PersistenceError::InvalidInput(format!("census missing {key}"))
                })?;
                let after = &rest[idx + marker.len()..];
                let end = after
                    .find(']')
                    .ok_or_else(|| PersistenceError::InvalidInput("census unterminated".into()))?;
                let items: Vec<String> = after[..end]
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                Ok((&after[end + 1..], items))
            }
            let mut obj = Vec::new();
            let mut rest = text;
            // Order matches the generated census JSON (index, table, trigger, view).
            for key in ["index", "table", "trigger", "view"] {
                let (next, items) = expect(rest, key)?;
                obj.push((key.to_owned(), items));
                rest = next;
            }
            Ok(Value { obj })
        }

        pub fn get(&self, key: &str) -> Option<&Vec<String>> {
            self.obj.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        }
    }
}

fn probe_fts(conn: &Connection, tokenizer: Option<&str>) -> Result<(), String> {
    let name = format!(
        "hrs010_fts_probe_{}",
        tokenizer.unwrap_or("base").replace(['_', '-'], "")
    );
    let sql = match tokenizer {
        None => format!("CREATE VIRTUAL TABLE temp.{name} USING fts5(x)"),
        Some(tok) => format!("CREATE VIRTUAL TABLE temp.{name} USING fts5(x, tokenize='{tok}')"),
    };
    let drop = format!("DROP TABLE IF EXISTS temp.{name}");
    // Upstream §2.6 probes by creating a temp virtual table.
    match conn.execute_batch(&sql) {
        Ok(()) => {
            let _ = conn.execute_batch(&drop);
            Ok(())
        }
        Err(e) => Err(format!("probe {tokenizer:?}: {e}")),
    }
}

/// Journal-mode resolution with verified read-back (§2.7/§4.1); no silent
/// coercion: every non-preferred outcome is recorded as degraded.
fn resolve_journal_mode(
    conn: &Connection,
    preference: JournalPreference,
    degraded: &mut Vec<String>,
) -> Result<JournalCapability, PersistenceError> {
    let linked = rusqlite::version();
    let vulnerable_walreset = {
        // Upstream's WAL-reset bug guard: linked SQLite in [3.50.0, 3.51.3)
        // must not use WAL. Parse "X.Y.Z".
        let parts: Vec<u32> = linked.split('.').filter_map(|p| p.parse().ok()).collect();
        if parts.len() >= 3 {
            let (maj, min, patch) = (parts[0], parts[1], parts[2]);
            maj == 3 && min == 50 && patch < 7
        } else {
            false
        }
    };
    if preference == JournalPreference::RollbackOnly || vulnerable_walreset {
        if vulnerable_walreset {
            degraded.push(format!(
                "journal resolved to delete: linked SQLite {linked} in WAL-reset bug range (upstream guard)"
            ));
        }
        conn.execute_batch("PRAGMA journal_mode = delete")
            .map_err(|e| classify(&e))?;
        return Ok(JournalCapability::RollbackOnly);
    }
    let current: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .map_err(|e| classify(&e))?;
    if current.eq_ignore_ascii_case("wal") {
        return Ok(JournalCapability::Wal);
    }
    // Switch to WAL with busy_timeout=0 semantics guarded by the caller's
    // exclusive open window (upstream `_set_journal_mode_no_wait`).
    conn.execute_batch("PRAGMA journal_mode = wal")
        .map_err(|e| classify(&e))?;
    let readback: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .map_err(|e| classify(&e))?;
    if readback.eq_ignore_ascii_case("wal") {
        Ok(JournalCapability::Wal)
    } else {
        degraded.push(format!(
            "journal switch to wal did not verify (read back {readback:?}); using delete"
        ));
        conn.execute_batch("PRAGMA journal_mode = delete")
            .map_err(|e| classify(&e))?;
        Ok(JournalCapability::RollbackOnly)
    }
}

pub struct StockSqliteBackend;

impl StockSqliteBackend {
    /// Open per §3.2: probe → quarantine admission → journal resolve →
    /// schema admission (version gate, additive reconciliation, deferred
    /// indexes, FTS probes) → capabilities + report.
    pub fn open(
        opts: OpenOptions,
    ) -> Result<(StockSqliteStore, super::api::OpenStore), PersistenceError> {
        let path = opts.path.clone();
        let mut report = OpenReport::default();
        let mut degraded: Vec<String> = Vec::new();

        if opts.read_only && !path.exists() {
            return Err(PersistenceError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("read-only open of missing store {}", path.display()),
            )));
        }
        if !opts.read_only && path.exists() && has_invalid_sqlite_header_preopen(&path)? {
            let quarantined = quarantine_invalid_state_db(&path)?;
            report.quarantined = Some(quarantined.display().to_string());
            degraded.push(format!(
                "quarantined invalid pre-open header to {}",
                report.quarantined.as_deref().unwrap_or("?")
            ));
        }

        let flags = if opts.read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_URI
        };
        let create = if opts.read_only {
            OpenFlags::empty()
        } else if opts.create_if_missing {
            OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::empty()
        };
        let conn = Connection::open_with_flags(&path, flags | create).map_err(|e| classify(&e))?;

        if !opts.read_only {
            conn.execute_batch(&format!(
                "PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 1000; PRAGMA journal_size_limit = {JOURNAL_SIZE_LIMIT_BYTES};"
            ))
            .map_err(|e| classify(&e))?;
        } else {
            conn.execute_batch("PRAGMA busy_timeout = 5000;")
                .map_err(|e| classify(&e))?;
        }

        // ── Schema admission (§3.6) ─────────────────────────────────────
        let have_version_table: bool = {
            let mut stmt = conn
                .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version'")
                .map_err(|e| classify(&e))?;
            stmt.exists([]).map_err(|e| classify(&e))?
        };
        let mut created_new_store = false;
        let version_before: Option<i64> = if have_version_table {
            conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(classify(&other)),
            })?
        } else {
            None
        };

        if !opts.read_only {
            if version_before.is_none() {
                // Fresh scaffold: apply the upstream DDL verbatim, idempotently.
                conn.execute_batch(UPSTREAM_SCHEMA_SQL)
                    .map_err(|e| classify(&e))?;
                conn.execute_batch(UPSTREAM_FTS_SQL)
                    .map_err(|e| classify(&e))?;
                conn.execute_batch(UPSTREAM_FTS_TRIGRAM_SQL)
                    .map_err(|e| classify(&e))?;
                conn.execute_batch(UPSTREAM_DEFERRED_INDEX_SQL)
                    .map_err(|e| classify(&e))?;
                conn.execute_batch(&format!(
                    "INSERT INTO schema_version (version) VALUES ({SCHEMA_VERSION_KNOWN});"
                ))
                .map_err(|e| classify(&e))?;
                report
                    .migrations_applied
                    .push("fresh scaffold (upstream DDL verbatim)".into());
                created_new_store = true;
            } else if version_before > Some(SCHEMA_VERSION_KNOWN) {
                // §3.6: newer files are never silently downgraded or rewritten.
                return Err(PersistenceError::Schema {
                    need: SchemaNeed::Newer(version_before.unwrap_or_default()),
                });
            }
            // FTS availability probes gate the FTS ensure steps (§2.6).
            let fts_base = probe_fts(&conn, None);
            let fts_trigram = probe_fts(&conn, Some("trigram"));
            if let Err(base_err) = fts_base {
                degraded.push(base_err);
            } else {
                conn.execute_batch(UPSTREAM_FTS_SQL)
                    .map_err(|e| classify(&e))?;
                if let Err(trigram_err) = fts_trigram {
                    degraded.push(trigram_err);
                } else {
                    conn.execute_batch(UPSTREAM_FTS_TRIGRAM_SQL)
                        .map_err(|e| classify(&e))?;
                }
            }
            conn.execute_batch(UPSTREAM_DEFERRED_INDEX_SQL)
                .map_err(|e| classify(&e))?;
            report
                .migrations_applied
                .push("deferred indexes + trigger reinstall (idempotent)".into());

            // Additive column reconciliation with upstream's own defaults.
            reconcile_columns(&conn, &mut report)?;

            let after: i64 = conn
                .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                    r.get(0)
                })
                .map_err(|e| classify(&e))?;
            if created_new_store {
                conn.execute_batch(&format!(
                    "INSERT OR IGNORE INTO state_meta (key, value) VALUES ('fts_storage_version', '{FTS_STORAGE_VERSION}');"
                ))
                .map_err(|e| classify(&e))?;
            }
            report.schema_version_before = version_before;
            report.schema_version_after = Some(after);
            if !fresh_census_matches(&conn)? {
                degraded.push("fresh-store census mismatch after scaffold".into());
            }
        } else {
            // Read-only admission: recognize, never repair.
            let v = version_before.ok_or(PersistenceError::Corrupt {
                scope: CorruptScope::MalformedSchema,
            })?;
            if v > SCHEMA_VERSION_KNOWN {
                return Err(PersistenceError::Schema {
                    need: SchemaNeed::Newer(v),
                });
            }
            report.schema_version_before = Some(v);
            report.schema_version_after = Some(v);
        }

        // ── Journal resolution ──────────────────────────────────────────
        let journal = if opts.read_only {
            JournalCapability::RollbackOnly
        } else {
            resolve_journal_mode(&conn, opts.journal, &mut degraded)?
        };
        report.degraded = degraded;

        // ── Capabilities ────────────────────────────────────────────────
        let fts = FtsCapabilities {
            base: probe_fts(&conn, None),
            trigram: probe_fts(&conn, Some("trigram")),
            cjk: probe_fts(&conn, Some("cjk_unicode61")),
        };
        let capabilities = Capabilities {
            journal,
            checkpoint_passive_only: true,
            advisory_generation_guard: false, // lockguard surface not ported in this slice
            file_format_standard_sqlite: true,
            bulk_parameter_limit: 999, // SQLITE_MAX_VARIABLE_NUMBER conservative floor (§2.4)
            fts,
        };

        let identity = file_identity(&path);
        let store = StockSqliteStore {
            path,
            conn: Mutex::new(conn),
            capabilities,
            report,
            identity,
            closed: AtomicBool::new(false),
        };
        let open = super::api::OpenStore {
            capabilities: store.capabilities.clone(),
            report: store.report.clone(),
        };
        Ok((store, open))
    }
}

fn reconcile_columns(conn: &Connection, report: &mut OpenReport) -> Result<(), PersistenceError> {
    use super::upstream_ddl::RECONCILE_COLUMNS;
    for (table, column, dflt) in RECONCILE_COLUMNS {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|e| classify(&e))?;
        let mut rows = stmt.query([]).map_err(|e| classify(&e))?;
        let mut present = false;
        while let Some(row) = rows.next().map_err(|e| classify(&e))? {
            let name: String = row.get(1).map_err(|e| classify(&e))?;
            if name == *column {
                present = true;
                break;
            }
        }
        drop(rows);
        drop(stmt);
        if !present {
            let ddl = match dflt {
                Some(d) => format!("ALTER TABLE {table} ADD COLUMN {column} {d}"),
                None => format!("ALTER TABLE {table} ADD COLUMN {column}"),
            };
            // Defaults come from upstream's own DDL; NULL for no-default columns.
            conn.execute_batch(&ddl).map_err(|e| classify(&e))?;
            report.columns_reconciled.push(format!("{table}.{column}"));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct StockSqliteStore {
    path: PathBuf,
    conn: Mutex<Connection>,
    capabilities: Capabilities,
    report: OpenReport,
    identity: Option<(u64, u64)>,
    closed: AtomicBool,
}

impl StockSqliteStore {
    pub fn open(
        opts: OpenOptions,
    ) -> Result<(StockSqliteStore, super::api::OpenStore), PersistenceError> {
        StockSqliteBackend::open(opts)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Slice-surface staleness read (§2.6): `fts_stale` marker present.
    pub fn fts_stale(&self) -> Result<bool, PersistenceError> {
        let conn = self.conn.lock().unwrap();
        let stale: Option<String> = conn
            .query_row(
                "SELECT value FROM state_meta WHERE key = 'fts_stale'",
                [],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
            .map_err(|e| classify(&e))?;
        Ok(stale.is_some())
    }

    fn assert_open(&self) -> Result<(), PersistenceError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(PersistenceError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "store closed",
            )));
        }
        Ok(())
    }
}

pub struct SqliteWriteTxn<'a> {
    conn: &'a Connection,
}

pub struct SqliteReadTxn<'a> {
    conn: &'a Connection,
}

/// Upstream `_stable_tool_key`/`_display_dedupe_key` analogue is NOT minted
/// by the slice writer (Python repr-tuple serialization is out of slice
/// scope; Python recompute paths heal display identities). Documented in
/// docs/evidence/hrs010/parity.md.
const INSERT_MESSAGE_SQL: &str = "INSERT INTO messages (session_id, role, content, tool_call_id,\
                   tool_calls, tool_name, effect_disposition, timestamp, token_count, finish_reason,\
                   reasoning, reasoning_content, reasoning_details, codex_reasoning_items,\
                   codex_message_items, platform_message_id, observed, _compressed_summary, active, api_content, display_kind,\
                   display_metadata, display_identity, message_uid, absorbed_message_uids, tool_call_uids,\
                   tool_call_uid)\
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, NULL,\
                   NULL, NULL, NULL, NULL,\
                   NULL, NULL, 0, 0, 1, NULL, NULL,\
                   NULL, NULL, ?9, NULL, NULL,\
                   NULL)";

fn tool_calls_count(tool_calls: &Option<String>) -> i64 {
    // Upstream `_tool_calls_count`: list length (JSON array), else 0 for
    // non-list. Slice binds JSON text; parse shallowly without serde.
    let Some(text) = tool_calls else { return 0 };
    let trimmed = text.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) || trimmed.len() <= 1 {
        return 0;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.trim().is_empty() {
        return 0;
    }
    // Count top-level commas outside strings (shallow JSON scanner).
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut count = 1usize;
    for ch in inner.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    count as i64
}

fn mint_message_uid(conn: &Connection) -> Result<String, PersistenceError> {
    // Upstream mint: lower(hex(randomblob(16))) — the same expression the
    // `messages_message_uid_insert` trigger uses (§2.8).
    conn.query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
        .map_err(|e| classify(&e))
}

impl<'a> WriteTxn for SqliteWriteTxn<'a> {
    fn ensure_session(
        &mut self,
        session_id: &str,
        source: &str,
        model: Option<&str>,
    ) -> Result<(), StoreError> {
        if session_id.is_empty() {
            return Err(StoreError::InvalidInput(
                "session id must not be empty".into(),
            ));
        }
        // Port of `_insert_session_row`'s upsert tail, slice columns: never
        // overwrite what an earlier writer set; the `source='unknown'`
        // placeholder is replaced by a real surface (#111999 upstream).
        self.conn
            .execute(
                "INSERT INTO sessions (id, source, created_source, model, started_at)
                 VALUES (?1, ?2, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET
                     source = CASE
                         WHEN sessions.source = 'unknown'
                         THEN COALESCE(excluded.source, 'unknown')
                         ELSE sessions.source
                     END,
                     model = COALESCE(sessions.model, excluded.model),
                     created_source = COALESCE(sessions.created_source, excluded.created_source)",
                rusqlite::params![session_id, source, model, now_epoch()],
            )
            .map_err(|e| match classify(&e) {
                PersistenceError::Io(io) => StoreError::InvalidInput(io.to_string()),
                other => StoreError::InvalidInput(other.to_string()),
            })
            .and(Ok(()))
    }

    fn append_message(&mut self, session_id: &str, msg: &AppendMessage) -> Result<i64, StoreError> {
        // Guard (inside the txn, §2.5(6)): refuse appends into a
        // compression-closed session (upstream
        // `_check_transcript_write_guards` → CompressionSessionClosedError).
        let ended: Option<(Option<f64>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT ended_at, end_reason FROM sessions WHERE id = ?1",
                [session_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
            .map_err(|_| StoreError::SessionRowMissing)?;
        let Some((ended_at, end_reason)) = ended else {
            return Err(StoreError::SessionRowMissing);
        };
        if ended_at.is_some() && end_reason.as_deref() == Some("compression") {
            return Err(StoreError::CompressionClosed);
        }

        let timestamp = msg.timestamp.unwrap_or_else(now_epoch);
        let uid = match &msg.message_uid {
            Some(u) => u.clone(),
            None => {
                mint_message_uid(self.conn).map_err(|e| StoreError::InvalidInput(e.to_string()))?
            }
        };
        self.conn
            .execute(
                INSERT_MESSAGE_SQL,
                rusqlite::params![
                    session_id,
                    msg.role,
                    msg.content,
                    msg.tool_call_id,
                    msg.tool_calls,
                    msg.tool_name,
                    timestamp,
                    msg.token_count,
                    uid,
                ],
            )
            .map_err(|e| match classify(&e) {
                PersistenceError::Io(io) => StoreError::InvalidInput(io.to_string()),
                _ => StoreError::SessionRowMissing,
            })?;
        let row_id = self.conn.last_insert_rowid();
        // Counter bump in the SAME transaction (§6.2).
        let calls = tool_calls_count(&msg.tool_calls);
        if calls > 0 {
            self.conn
                .execute(
                    "UPDATE sessions SET message_count = message_count + 1,
                       tool_call_count = tool_call_count + ?1 WHERE id = ?2",
                    rusqlite::params![calls, session_id],
                )
                .map_err(|_| StoreError::InvalidInput("counter bump failed".into()))?;
        } else {
            self.conn
                .execute(
                    "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
                    [session_id],
                )
                .map_err(|_| StoreError::InvalidInput("counter bump failed".into()))?;
        }
        Ok(row_id)
    }

    fn end_session(
        &mut self,
        session_id: &str,
        end_reason: &str,
    ) -> Result<EndSessionOutcome, StoreError> {
        // First end_reason wins (`ended_at IS NULL` predicate, §2.4).
        let changed = self
            .conn
            .execute(
                "UPDATE sessions SET ended_at = ?1, end_reason = ?2
                 WHERE id = ?3 AND ended_at IS NULL",
                rusqlite::params![now_epoch(), end_reason, session_id],
            )
            .map_err(|_| StoreError::InvalidInput("end_session update failed".into()))?;
        if changed == 0 {
            return Ok(EndSessionOutcome::AlreadyEnded);
        }
        // Generation bump ONLY when this call actually wrote the boundary
        // (`_end_and_bump` + `_bump_conversation_generation`): reset-class
        // reasons only, and never GC'd / never decremented (§2.2).
        let bumped = if RESET_END_REASONS.contains(&end_reason) {
            let row: Option<(Option<String>, Option<String>)> = self
                .conn
                .query_row(
                    "SELECT source, session_key FROM sessions WHERE id = ?1",
                    [session_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })
                .map_err(|_| StoreError::InvalidInput("generation lookup failed".into()))?;
            if let Some((source, key)) = row {
                let (source, key) = (source.unwrap_or_default(), key.unwrap_or_default());
                if !source.trim().is_empty() && !key.trim().is_empty() {
                    self.conn
                        .execute(
                            "INSERT INTO conversation_generations (source, session_key, generation)
                             VALUES (?1, ?2, 1)
                             ON CONFLICT(source, session_key) DO UPDATE
                                 SET generation = conversation_generations.generation + 1",
                            rusqlite::params![source, key],
                        )
                        .map_err(|_| StoreError::InvalidInput("generation bump failed".into()))?;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        Ok(EndSessionOutcome::Ended {
            generation_bumped: bumped,
        })
    }

    fn record_model_usage(
        &mut self,
        session_id: &str,
        delta: &ModelUsageDelta,
    ) -> Result<(), StoreError> {
        // Row-existence guard with the `unknown` placeholder (upstream
        // `update_token_counts` first statement).
        self.ensure_session(session_id, "unknown", Some(&delta.model))?;
        let has_usage = delta.input_tokens != 0
            || delta.output_tokens != 0
            || delta.cache_read_tokens != 0
            || delta.cache_write_tokens != 0
            || delta.reasoning_tokens != 0
            || delta.api_call_count != 0
            || delta.estimated_cost_usd != 0.0;
        // First-accounted-route repair (upstream `_do` first UPDATE).
        if delta.task.is_empty()
            && has_usage
            && !delta.model.is_empty()
            && !delta.billing_provider.is_empty()
        {
            let existing: Option<(Option<String>, Option<String>, i64)> = self
                .conn
                .query_row(
                    "SELECT model, billing_provider, COALESCE(api_call_count, 0) FROM sessions WHERE id = ?1",
                    [session_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })
                .map_err(|_| StoreError::InvalidInput("usage route lookup failed".into()))?;
            if let Some((model, provider, calls)) = existing {
                if calls == 0
                    && (model.as_deref() != Some(delta.model.as_str())
                        || provider.as_deref() != Some(delta.billing_provider.as_str()))
                {
                    self.conn
                        .execute(
                            "UPDATE sessions SET model = ?1, billing_provider = ?2,
                               billing_base_url = ?3, billing_mode = ?4 WHERE id = ?5",
                            rusqlite::params![
                                delta.model,
                                delta.billing_provider,
                                delta.billing_base_url,
                                delta.billing_mode,
                                session_id
                            ],
                        )
                        .map_err(|_| StoreError::InvalidInput("route repair failed".into()))?;
                }
            }
        }
        self.conn
            .execute(
                "UPDATE sessions SET
                       input_tokens = COALESCE(input_tokens, 0) + ?1,
                       output_tokens = COALESCE(output_tokens, 0) + ?2,
                       cache_read_tokens = COALESCE(cache_read_tokens, 0) + ?3,
                       cache_write_tokens = COALESCE(cache_write_tokens, 0) + ?4,
                       reasoning_tokens = COALESCE(reasoning_tokens, 0) + ?5,
                       estimated_cost_usd = COALESCE(estimated_cost_usd, 0) + COALESCE(?6, 0),
                       cost_status = COALESCE(?7, cost_status),
                       cost_source = COALESCE(?8, cost_source),
                       billing_provider = COALESCE(billing_provider, ?9),
                       billing_base_url = COALESCE(billing_base_url, ?10),
                       billing_mode = COALESCE(billing_mode, ?11),
                       model = COALESCE(model, ?12),
                       api_call_count = COALESCE(api_call_count, 0) + ?13
                   WHERE id = ?14",
                rusqlite::params![
                    delta.input_tokens,
                    delta.output_tokens,
                    delta.cache_read_tokens,
                    delta.cache_write_tokens,
                    delta.reasoning_tokens,
                    if has_usage {
                        Some(delta.estimated_cost_usd)
                    } else {
                        None
                    },
                    delta.cost_status,
                    delta.cost_source,
                    if has_usage {
                        Some(delta.billing_provider.as_str())
                    } else {
                        None
                    },
                    if has_usage {
                        Some(delta.billing_base_url.as_str())
                    } else {
                        None
                    },
                    if has_usage {
                        Some(delta.billing_mode.as_str())
                    } else {
                        None
                    },
                    if has_usage {
                        Some(delta.model.as_str())
                    } else {
                        None
                    },
                    delta.api_call_count,
                    session_id,
                ],
            )
            .map_err(|_| StoreError::InvalidInput("token update failed".into()))?;
        // Per-model attribution upsert (upstream `_MODEL_USAGE_UPSERT_SQL`);
        // missing model/provider falls back to the session row for main-loop
        // rows (task=''), stays 'unknown'/empty for aux rows (§ usage).
        let (model, provider, base_url, mode) = if delta.task.is_empty() {
            let row: (Option<String>, Option<String>, Option<String>, Option<String>) = self
                .conn
                .query_row(
                    "SELECT model, billing_provider, billing_base_url, billing_mode FROM sessions WHERE id = ?1",
                    [session_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap_or((None, None, None, None));
            (
                if delta.model.is_empty() {
                    row.0.unwrap_or_else(|| "unknown".into())
                } else {
                    delta.model.clone()
                },
                if delta.billing_provider.is_empty() {
                    row.1.unwrap_or_default()
                } else {
                    delta.billing_provider.clone()
                },
                if delta.billing_base_url.is_empty() {
                    row.2.unwrap_or_default()
                } else {
                    delta.billing_base_url.clone()
                },
                if delta.billing_mode.is_empty() {
                    row.3.unwrap_or_default()
                } else {
                    delta.billing_mode.clone()
                },
            )
        } else {
            (
                if delta.model.is_empty() {
                    "unknown".into()
                } else {
                    delta.model.clone()
                },
                delta.billing_provider.clone(),
                delta.billing_base_url.clone(),
                delta.billing_mode.clone(),
            )
        };
        let now = now_epoch();
        self.conn
            .execute(
                "INSERT INTO session_model_usage (
                       session_id, model, billing_provider, billing_base_url, billing_mode,
                       task, api_call_count, input_tokens, output_tokens,
                       cache_read_tokens, cache_write_tokens, reasoning_tokens,
                       estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
                       first_seen, last_seen
                   ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0.0, ?14, ?15, ?16, ?17)
                   ON CONFLICT(session_id, model, billing_provider, billing_base_url, billing_mode, task)
                   DO UPDATE SET
                       api_call_count = api_call_count + excluded.api_call_count,
                       input_tokens = input_tokens + excluded.input_tokens,
                       output_tokens = output_tokens + excluded.output_tokens,
                       cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
                       cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
                       reasoning_tokens = reasoning_tokens + excluded.reasoning_tokens,
                       estimated_cost_usd = estimated_cost_usd + excluded.estimated_cost_usd,
                       actual_cost_usd = actual_cost_usd + excluded.actual_cost_usd,
                       cost_status = COALESCE(excluded.cost_status, cost_status),
                       cost_source = COALESCE(excluded.cost_source, cost_source),
                       last_seen = excluded.last_seen",
                rusqlite::params![
                    session_id,
                    model,
                    provider,
                    base_url,
                    mode,
                    delta.task,
                    delta.api_call_count,
                    delta.input_tokens,
                    delta.output_tokens,
                    delta.cache_read_tokens,
                    delta.cache_write_tokens,
                    delta.reasoning_tokens,
                    delta.estimated_cost_usd,
                    delta.cost_status,
                    delta.cost_source,
                    now,
                    now,
                ],
            )
            .map_err(|_| StoreError::InvalidInput("model usage upsert failed".into()))?;
        Ok(())
    }
}

fn sanitize_fts5_query(query: &str) -> String {
    // Port of `_sanitize_fts5_query` (§2.6). Cap first.
    let query = &query.chars().take(MAX_FTS5_QUERY_CHARS).collect::<String>();
    // 1. Protect balanced quoted phrases.
    let mut quoted: Vec<String> = Vec::new();
    let mut sanitized = String::new();
    let bytes: Vec<char> = query.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '"' {
            if let Some(close) = bytes[i + 1..].iter().position(|&c| c == '"') {
                let end = i + 1 + close;
                let phrase: String = bytes[i..=end].iter().collect();
                sanitized.push_str(&format!("\u{0}Q{}\u{0}", quoted.len()));
                quoted.push(phrase);
                i = end + 1;
                continue;
            }
        }
        sanitized.push(bytes[i]);
        i += 1;
    }
    let sanitized = sanitized.replace('"', " ");
    // 2. Strip FTS5-special characters.
    const SPECIAL: &[char] = &[
        '+', '{', '}', '(', ')', ':', '"', '^', '@', '/', '#', '&', '|', '~', '[', ']', '<', '>',
        ',', ';', '!', '?', '$', '=', '\\', '\'',
    ];
    let mut sanitized: String = sanitized.chars().filter(|c| !SPECIAL.contains(c)).collect();
    if !sanitized.chars().any(|c| {
        let cp = c as u32;
        (0x4E00..=0x9FFF).contains(&cp) || (0x3400..=0x4DBF).contains(&cp)
    }) {
        sanitized = sanitized.replace('%', " ");
    }
    // 3. Collapse repeated '*' and drop leading '*'.
    let mut collapsed = String::new();
    let mut last_star = false;
    for c in sanitized.chars() {
        if c == '*' {
            if !last_star {
                collapsed.push(c);
            }
            last_star = true;
        } else {
            collapsed.push(c);
            last_star = false;
        }
    }
    let mut sanitized = collapsed;
    while sanitized.starts_with('*') || sanitized.starts_with(" *") {
        sanitized = sanitized[1..].to_string();
    }
    // 4. Drop dangling boolean operators at start/end.
    let mut trimmed = sanitized.trim().to_string();
    for op in ["AND", "OR", "NOT"] {
        let lower = trimmed.to_lowercase();
        if lower.starts_with(&op.to_lowercase()) {
            let rest = trimmed[op.len()..].trim_start().to_string();
            trimmed = rest;
        }
        let lower = trimmed.to_lowercase();
        if lower.ends_with(&op.to_lowercase()) {
            let cut = trimmed.len() - op.len();
            trimmed = trimmed[..cut].trim_end().to_string();
        }
    }
    // 5. Quote dotted/hyphenated/underscored terms in one pass.
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        let dotted = word.contains('.') || word.contains('-') || word.contains('_');
        let complex = word.chars().any(|c| c == '.' || c == '-' || c == '_');
        if complex && word.chars().any(|c| c.is_alphanumeric()) && dotted {
            out.push('"');
            out.push_str(word);
            out.push('"');
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in trimmed.chars() {
        if c.is_whitespace() {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    // 6. Restore preserved quoted phrases.
    let mut restored = out;
    for (idx, phrase) in quoted.iter().enumerate() {
        let marker = format!("\u{0}Q{}\u{0}", idx);
        restored = restored.replace(&marker, phrase);
    }
    restored.trim().to_string()
}

impl<'a> ReadTxn for SqliteReadTxn<'a> {
    fn get_messages(
        &mut self,
        session_id: &str,
        include_inactive: bool,
        include_compacted: bool,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<Vec<MessageRow>, StoreError> {
        // §6.2: `messages.id` ascending; active/compacted inclusion flags
        // (upstream `_active_clause`).
        let active_clause = if include_inactive {
            ""
        } else if include_compacted {
            " AND (active = 1 OR compacted = 1)"
        } else {
            " AND active = 1"
        };
        let sql = format!(
            "SELECT id, session_id, role, content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, message_uid, active, compacted, display_kind
             FROM messages WHERE session_id = ?1{active_clause}
             ORDER BY id ASC LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| match classify(&e) {
            PersistenceError::Corrupt { scope } => {
                StoreError::InvalidInput(format!("corrupt {scope:?}"))
            }
            _ => StoreError::InvalidInput("get_messages prepare failed".into()),
        })?;
        let rows = stmt
            .query_map(
                rusqlite::params![
                    session_id,
                    limit.map(|l| l as i64).unwrap_or(-1),
                    offset as i64
                ],
                |r| {
                    Ok(MessageRow {
                        id: r.get(0)?,
                        session_id: r.get(1)?,
                        role: r.get(2)?,
                        content: r.get(3)?,
                        tool_call_id: r.get(4)?,
                        tool_calls: r.get(5)?,
                        tool_name: r.get(6)?,
                        timestamp: r.get(7)?,
                        token_count: r.get(8)?,
                        message_uid: r.get(9)?,
                        active: r.get::<_, i64>(10)? != 0,
                        compacted: r.get::<_, i64>(11)? != 0,
                        display_kind: r.get(12)?,
                    })
                },
            )
            .map_err(|_| StoreError::InvalidInput("get_messages query failed".into()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn get_session(&mut self, session_id: &str) -> Result<Option<Session>, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, source, model, billing_provider, billing_base_url, billing_mode,
                        message_count, tool_call_count, input_tokens,
                        output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                        api_call_count, estimated_cost_usd, started_at, ended_at, end_reason,
                        parent_session_id, session_key
                 FROM sessions WHERE id = ?1",
                [session_id],
                |r| {
                    Ok(Session {
                        id: r.get(0)?,
                        source: r.get(1)?,
                        model: r.get(2)?,
                        billing_provider: r.get(3)?,
                        billing_base_url: r.get(4)?,
                        billing_mode: r.get(5)?,
                        message_count: r.get(6)?,
                        tool_call_count: r.get(7)?,
                        input_tokens: r.get(8)?,
                        output_tokens: r.get(9)?,
                        cache_read_tokens: r.get(10)?,
                        cache_write_tokens: r.get(11)?,
                        reasoning_tokens: r.get(12)?,
                        api_call_count: r.get(13)?,
                        estimated_cost_usd: r.get(14)?,
                        started_at: r.get(15)?,
                        ended_at: r.get(16)?,
                        end_reason: r.get(17)?,
                        parent_session_id: r.get(18)?,
                        session_key: r.get(19)?,
                    })
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
            .map_err(|_| StoreError::InvalidInput("get_session failed".into()))?;
        Ok(row)
    }

    fn search_messages(
        &mut self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        // Staleness markers refuse FTS silently — LIKE fallback serves
        // canonical rows (§2.6 degraded path, same as upstream's LIKE arm).
        let stale: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM state_meta WHERE key IN ('fts_stale', 'fts_cjk_stale', 'fts_rebuild_deferral')",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        let sanitized = sanitize_fts5_query(query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }
        // LIKE fallbacks scan canonical rows: LIKE has no FTS5 grammar, so
        // the RAW term is the anchor (upstream `_LIKE_SNIPPET_SQL` anchors on
        // the user term; the sanitizer's quoting is MATCH-syntax only).
        let like_term: String = query.chars().take(MAX_FTS5_QUERY_CHARS).collect();
        let like_term = like_term.trim().trim_matches('"').to_string();
        let visibility =
            "(m.active = 1 OR m.compacted = 1) AND COALESCE(m.display_kind, '') <> 'hidden'";
        if stale {
            let sql = format!(
                "SELECT m.id, m.session_id, m.role, m.timestamp
                 FROM messages m WHERE m.content LIKE '%' || ?1 || '%' AND {visibility}
                 ORDER BY m.timestamp DESC LIMIT ?2 OFFSET ?3"
            );
            let mut stmt = self.conn.prepare(&sql).map_err(|_| {
                StoreError::InvalidInput("search (stale fallback) prepare failed".into())
            })?;
            let rows = stmt
                .query_map(
                    rusqlite::params![like_term, limit as i64, offset as i64],
                    |r| {
                        Ok(SearchHit {
                            message_id: r.get(0)?,
                            session_id: r.get(1)?,
                            role: r.get(2)?,
                            snippet: String::new(),
                            rank: 0.0,
                        })
                    },
                )
                .map_err(|_| StoreError::InvalidInput("search (stale fallback) failed".into()))?;
            return Ok(rows.filter_map(|r| r.ok()).collect());
        }
        let sql = format!(
            "SELECT m.id, m.session_id, m.role,
                    snippet(messages_fts, -1, '>>>', '<<<', '...', 40) AS snippet, rank
             FROM messages_fts
             JOIN messages m ON m.id = messages_fts.rowid
             JOIN sessions s ON s.id = m.session_id
             WHERE messages_fts MATCH ?1 AND {visibility}
             ORDER BY rank LIMIT ?2 OFFSET ?3"
        );
        match self.conn.prepare(&sql) {
            Err(e) => match classify(&e) {
                PersistenceError::Corrupt {
                    scope: CorruptScope::FtsIndex,
                } => {
                    // FTS-scoped corruption is repairable at the index layer;
                    // the slice answers from canonical rows (fail-open arm).
                    self.like_fallback(&like_term, limit, offset)
                }
                _ => Err(StoreError::InvalidInput("search prepare failed".into())),
            },
            Ok(mut stmt) => {
                let rows = stmt.query_map(
                    rusqlite::params![sanitized, limit as i64, offset as i64],
                    |r| {
                        Ok(SearchHit {
                            message_id: r.get(0)?,
                            session_id: r.get(1)?,
                            role: r.get(2)?,
                            snippet: r.get(3)?,
                            rank: r.get(4)?,
                        })
                    },
                );
                match rows {
                    Ok(rows) => {
                        let collected: Vec<SearchHit> = rows.filter_map(|r| r.ok()).collect();
                        Ok(collected)
                    }
                    Err(e) => match classify(&e) {
                        PersistenceError::Corrupt {
                            scope: CorruptScope::FtsIndex,
                        } => self.like_fallback(&like_term, limit, offset),
                        // FTS5 syntax error despite sanitization → zero
                        // results (upstream OperationalError arm).
                        _ => Ok(Vec::new()),
                    },
                }
            }
        }
    }
}

impl<'a> SqliteReadTxn<'a> {
    fn like_fallback(
        &mut self,
        term: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let sql = "SELECT m.id, m.session_id, m.role FROM messages m
                   WHERE m.content LIKE '%' || ?1 || '%' ORDER BY m.id LIMIT ?2 OFFSET ?3";
        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|_| StoreError::InvalidInput("search fallback failed".into()))?;
        let rows = stmt
            .query_map(rusqlite::params![term, limit as i64, offset as i64], |r| {
                Ok(SearchHit {
                    message_id: r.get(0)?,
                    session_id: r.get(1)?,
                    role: r.get(2)?,
                    snippet: String::new(),
                    rank: 0.0,
                })
            })
            .map_err(|_| StoreError::InvalidInput("search fallback failed".into()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

impl SessionStore for StockSqliteStore {
    fn write<T>(
        &self,
        patience: Patience,
        f: &mut dyn FnMut(&mut dyn WriteTxn) -> Result<T, StoreError>,
    ) -> Result<T, PersistenceError> {
        self.assert_open()?;
        // File-identity probe inside the write path (§2.11): an out-of-band
        // replace (mv/cp) cannot limp through in-place surgery.
        if let Some(current) = file_identity(&self.path) {
            if Some(current) != self.identity {
                return Err(PersistenceError::Replaced);
            }
        }
        let mut conn = self.conn.lock().unwrap();
        let deadline = Instant::now() + patience.0;
        let mut entry_ioerr_retried = false;
        loop {
            let begin_result = conn.execute_batch("BEGIN IMMEDIATE");
            if let Err(e) = begin_result {
                let classified = classify(&e);
                // §3.3(4): I/O error at transaction entry is
                // retryable-once — the callback has not run.
                if is_busy(&classified) && Instant::now() < deadline {
                    drop(conn);
                    jitter_sleep();
                    conn = self.conn.lock().unwrap();
                    continue;
                }
                if matches!(classified, PersistenceError::Io(_)) && !entry_ioerr_retried {
                    entry_ioerr_retried = true;
                    drop(conn);
                    jitter_sleep();
                    conn = self.conn.lock().unwrap();
                    continue;
                }
                return Err(classified);
            }
            let mut txn = SqliteWriteTxn { conn: &conn };
            match f(&mut txn) {
                Ok(value) => match conn.execute_batch("COMMIT") {
                    Ok(()) => return Ok(value),
                    Err(e) => {
                        let _ = conn.execute_batch("ROLLBACK");
                        let classified = classify(&e);
                        if is_busy(&classified) && Instant::now() < deadline {
                            drop(conn);
                            jitter_sleep();
                            conn = self.conn.lock().unwrap();
                            continue;
                        }
                        return Err(classified);
                    }
                },
                Err(store_err) => {
                    // Any error path rolls back before returning (§3.3(3)).
                    let _ = conn.execute_batch("ROLLBACK");
                    return Err(match store_err {
                        StoreError::LeaseLost { lease } => PersistenceError::LeaseLost { lease },
                        StoreError::SessionRowMissing => PersistenceError::SessionRowMissing,
                        StoreError::CompressionClosed => PersistenceError::CompressionClosed,
                        StoreError::InvalidInput(m) => PersistenceError::InvalidInput(m),
                    });
                }
            }
        }
    }

    fn read<T>(
        &self,
        f: &mut dyn FnMut(&mut dyn ReadTxn) -> Result<T, StoreError>,
    ) -> Result<T, PersistenceError> {
        self.assert_open()?;
        let conn = self.conn.lock().unwrap();
        let mut txn = SqliteReadTxn { conn: &conn };
        f(&mut txn).map_err(|e| match e {
            StoreError::LeaseLost { lease } => PersistenceError::LeaseLost { lease },
            StoreError::SessionRowMissing => PersistenceError::SessionRowMissing,
            StoreError::CompressionClosed => PersistenceError::CompressionClosed,
            StoreError::InvalidInput(m) => PersistenceError::InvalidInput(m),
        })
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn open_report(&self) -> &OpenReport {
        &self.report
    }

    fn checkpoint_passive(&self) -> Result<(), PersistenceError> {
        // §2.7/§4.2: PASSIVE only at runtime; TRUNCATE is forbidden here.
        self.assert_open()?;
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
            .map_err(|e| classify(&e))
            .and(Ok(()))
    }

    fn backup(&self, dest: &Path) -> Result<(), PersistenceError> {
        // Backup-API snapshot (§2.10): copying only the main file loses WAL
        // data; the API copies page-consistent state.
        self.assert_open()?;
        if dest.exists() {
            return Err(PersistenceError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("backup destination exists: {}", dest.display()),
            )));
        }
        let src = self.conn.lock().unwrap();
        let step = {
            let mut dst = Connection::open(dest).map_err(|e| classify(&e))?;
            let backup = rusqlite::backup::Backup::new(&src, &mut dst).map_err(|e| classify(&e))?;
            backup.run_to_completion(64, Duration::from_millis(5), None)
        };
        if let Err(e) = step {
            let _ = std::fs::remove_file(dest); // fail closed: no partial artifact
            return Err(classify(&e));
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), PersistenceError> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        // Clean close lets SQLite unlink WAL sidecars (§2.1); acknowledged
        // writes must be observable after reopen (F-3 at slice scale).
        let conn = self.conn.lock().map_err(|_| {
            PersistenceError::Io(std::io::Error::other("close: writer lock poisoned"))
        })?;
        drop(conn);
        Ok(())
    }
}

/// Capability-convenience: run an operation the capability query already
/// ruled out returns `Unsupported` (§3.2 rules) — used by callers for the
/// optional paths (§3.7(3)).
pub fn require(capabilities: &Capabilities, c: Capability) -> Result<(), PersistenceError> {
    match capabilities.supports(c) {
        CapabilitySupport::Yes => Ok(()),
        CapabilitySupport::No { reason } | CapabilitySupport::Degraded { reason } => {
            let _ = reason;
            Err(PersistenceError::Unsupported { capability: c })
        }
    }
}
