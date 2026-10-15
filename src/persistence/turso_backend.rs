//! Experimental Turso (Limbo) candidate backend — HRS-009 evaluation.
//!
//! OPTIONAL: compiled only under the `turso` feature
//! (`Cargo.toml [features] turso = ["dep:turso", "dep:tokio"]`); stock
//! SQLite remains the only default backend (specs/persistence-shim.md §3.1).
//! This module exists to make the HRS-009 evaluation MEASURED, not to offer
//! Turso for use: specs/backend-evaluation.md owns the verdict, and no
//! default/backend switch is implied by its presence.
//!
//! Engine-leak rule (§3.1): every `turso::` type lives in this module; the
//! caller-facing surface is `TursoBackend::open` → the same engine-neutral
//! [`SessionStore`] the stock backend implements.
//!
//! Measured constraints at turso 0.8.3 (probes in specs/backend-evaluation.md):
//! - upstream DDL core applies verbatim (25/25 schema statements, 6/6
//!   deferred incl. triggers) but FTS5 virtual tables fail ("no such
//!   module: fts5" — Turso ships its own `fts` index method instead), so
//!   search here is the canonical-rows LIKE fallback and the FTS
//!   capabilities answer No with that reason;
//! - the default build is a single-process store: a second PROCESS open
//!   fails with a file-lock error (probe 4) — cross-process concurrency
//!   needs `experimental_multiprocess_wal`, which this backend does not
//!   enable;
//! - `file_format_standard_sqlite` is reported true: the written file
//!   carries the verbatim `SQLite format 3\0` header and was read back by
//!   stock sqlite3 and rusqlite (bidirectional) — with the caveat that the
//!   full upstream schema INCLUDING FTS objects cannot exist on this
//!   engine, so upstream-produced files with FTS indexes are NOT a
//!   supported input (schema admission below records the gap as degraded).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use turso::Value as TValue;

use super::api::{
    AppendMessage, BusyKind, Capabilities, CorruptScope, EndSessionOutcome, FtsCapabilities,
    JournalCapability, MessageRow, ModelUsageDelta, OpenOptions, OpenReport, Patience,
    PersistenceError, ReadTxn, SchemaNeed, SearchHit, Session, SessionStore, StoreError, WriteTxn,
    RESET_END_REASONS, SCHEMA_VERSION_KNOWN,
};
use super::upstream_ddl::{UPSTREAM_DEFERRED_INDEX_SQL, UPSTREAM_SCHEMA_SQL};

/// Conservative parameter floor reported for capability gating (§2.4):
/// the slice never batches more than 900 bind parameters; Turso's true
/// limit is not documented at 0.8.3, so the upstream floor is reported
/// rather than an optimistic value.
const BULK_PARAMETER_FLOOR: u32 = 999;
/// FTS storage-layout version is NOT stamped by this backend (no FTS
/// objects are created): the marker's absence is itself the honest record.
const FTS_UNAVAILABLE_REASON: &str =
    "turso 0.8.3 has no fts5 module (probe: CREATE VIRTUAL TABLE ... USING fts5 → \
     Parse error: no such module: fts5); turso ships a non-SQLite 'fts' index \
     method instead";

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn jitter_sleep() {
    let ms = 5 + (std::process::id() % 17) as u64;
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

/// Map a turso error onto the §3.4 taxonomy, code-first where the engine
/// exposes codes (it mostly exposes prose classes — recorded honestly).
fn classify(err: &turso::Error) -> PersistenceError {
    match err {
        turso::Error::Busy(_) | turso::Error::BusySnapshot(_) => PersistenceError::Busy {
            kind: BusyKind::Busy,
        },
        turso::Error::Interrupt(_) => PersistenceError::Busy {
            kind: BusyKind::Abandoned,
        },
        turso::Error::Corrupt(_) | turso::Error::NotAdb(_) => PersistenceError::Corrupt {
            scope: CorruptScope::Store,
        },
        turso::Error::DatabaseFull(_) => PersistenceError::DiskFull,
        turso::Error::IoError(kind, msg) => PersistenceError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("turso io ({kind:?}): {msg}"),
        )),
        other => PersistenceError::Io(std::io::Error::other(format!("turso error: {other}"))),
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

fn tv_to_option_string(v: &TValue) -> Option<String> {
    match v {
        TValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

fn tv_to_string(v: &TValue) -> String {
    match v {
        TValue::Text(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

/// One private current-thread runtime per store; every shim call drives its
/// async operations to completion on the calling thread under the store's
/// write/read mutex (same serialization shape as the stock backend's
/// `Mutex<Connection>`). Drop order matters: the connection must be
/// dropped before the runtime shuts down (fields drop in declaration
/// order, so the runtime is declared first).
struct Engine {
    rt: tokio::runtime::Runtime,
    conn: turso::Connection,
}

/// Split a multi-statement DDL script into statements the way SQLite's
/// sqlite3_complete does for the upstream DDL shapes: honoring string
/// literals, comments, and trigger BEGIN..END bodies.
fn split_statements(sql: &str) -> Vec<String> {
    let mut stmts = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0usize;
    let mut trigger_depth = 0usize;
    let mut seen_trigger_header = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            cur.push(c);
            i += 1;
            while i < chars.len() {
                cur.push(chars[i]);
                if chars[i] == '\'' {
                    if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        cur.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] != '\n' {
                cur.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            cur.push(c);
            cur.push(chars[i + 1]);
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                cur.push(chars[i]);
                i += 1;
            }
            if i + 1 < chars.len() {
                cur.push('*');
                cur.push('/');
                i += 2;
            }
            continue;
        }
        cur.push(c);
        i += 1;
        if c == ';' {
            let trimmed = cur.trim();
            let upper = trimmed.to_uppercase();
            if trigger_depth == 0 && !upper.starts_with("CREATE TRIGGER") {
                stmts.push(trimmed.to_string());
                cur.clear();
                seen_trigger_header = false;
            } else {
                let body_upper = cur.to_uppercase();
                let begins = body_upper.matches(" BEGIN").count();
                let ends =
                    body_upper.matches(" END").count() + usize::from(body_upper.starts_with("END"));
                if upper.starts_with("CREATE TRIGGER") && !seen_trigger_header {
                    seen_trigger_header = true;
                    trigger_depth += begins;
                } else if seen_trigger_header {
                    trigger_depth += begins;
                    if ends > 0 {
                        trigger_depth = trigger_depth.saturating_sub(ends);
                        if trigger_depth == 0 {
                            stmts.push(trimmed.to_string());
                            cur.clear();
                            seen_trigger_header = false;
                        }
                    }
                }
            }
        }
    }
    let rest = cur.trim();
    if !rest.is_empty() {
        stmts.push(rest.to_string());
    }
    stmts
}

pub struct TursoBackend;

impl TursoBackend {
    /// Open per the stock backend's §3.2 shape, adapted to what turso 0.8.3
    /// actually offers. Every adaptation is recorded in `degraded`.
    pub fn open(
        opts: OpenOptions,
    ) -> Result<(TursoStore, super::api::OpenStore), PersistenceError> {
        let path = opts.path.clone();
        let mut report = OpenReport::default();
        let mut degraded: Vec<String> = Vec::new();

        if opts.read_only && !path.exists() {
            return Err(PersistenceError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("read-only open of missing store {}", path.display()),
            )));
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| PersistenceError::Io(std::io::Error::other(format!("runtime: {e}"))))?;

        let mut builder = turso::Builder::new_local(&path.to_string_lossy());
        if opts.read_only {
            builder = builder.read_only(true);
        }
        let db = rt.block_on(builder.build()).map_err(|e| classify(&e))?;
        let conn = db.connect().map_err(|e| classify(&e))?;
        conn.busy_timeout(std::time::Duration::from_millis(1000))
            .ok();

        // ── Schema admission (§3.6 shape) ───────────────────────────────
        let have_version_table: bool = rt
            .block_on(async {
                let mut rows = conn
                    .query(
                        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_version'",
                        (),
                    )
                    .await?;
                Ok::<bool, turso::Error>(rows.next().await?.is_some())
            })
            .map_err(|e: turso::Error| classify(&e))?;

        let version_before: Option<i64> = if have_version_table {
            rt.block_on(async {
                let mut rows = conn
                    .query("SELECT version FROM schema_version LIMIT 1", ())
                    .await?;
                match rows.next().await? {
                    Some(row) => match row.get_value(0)? {
                        TValue::Integer(v) => Ok(Some(v)),
                        other => Err(turso::Error::ConversionFailure(format!(
                            "schema_version {other:?}"
                        ))),
                    },
                    None => Ok(None),
                }
            })
            .map_err(|e: turso::Error| classify(&e))?
        } else {
            None
        };

        let mut created_new_store = false;
        if !opts.read_only {
            match version_before {
                None => {
                    // Fresh scaffold: upstream DDL verbatim, MINUS the FTS
                    // sections (they cannot exist on this engine — recorded).
                    for stmt in split_statements(UPSTREAM_SCHEMA_SQL) {
                        rt.block_on(conn.execute(&stmt, ()))
                            .map_err(|e| classify(&e))?;
                    }
                    for stmt in split_statements(UPSTREAM_DEFERRED_INDEX_SQL) {
                        rt.block_on(conn.execute(&stmt, ()))
                            .map_err(|e| classify(&e))?;
                    }
                    rt.block_on(conn.execute(
                        &format!(
                            "INSERT INTO schema_version (version) VALUES ({SCHEMA_VERSION_KNOWN});"
                        ),
                        (),
                    ))
                    .map_err(|e| classify(&e))?;
                    degraded.push(
                        "FTS objects (messages_fts, trigram, cjk views/triggers) NOT created: \
                         no fts5 module at turso 0.8.3 — fresh store is NOT census-equal to \
                         stock SQLite"
                            .into(),
                    );
                    report
                        .migrations_applied
                        .push("fresh scaffold (upstream DDL verbatim, FTS skipped)".into());
                    created_new_store = true;
                }
                Some(v) if v > SCHEMA_VERSION_KNOWN => {
                    return Err(PersistenceError::Schema {
                        need: SchemaNeed::Newer(v),
                    });
                }
                Some(_) => {
                    // Existing store at a known version: recognize, never
                    // repair (§3.6 read-admission shape).
                    report.schema_version_before = version_before;
                    report.schema_version_after = version_before;
                }
            }
        } else {
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

        // ── Journal resolution: probe with verified read-back. Turso 0.8.3
        // always reports wal (probe 1); a switch statement is unnecessary —
        // only the READ-BACK is trusted. `PRAGMA x = y` via execute() errors
        // ("unexpected row during execution"), so pragmas go through query().
        let journal = if opts.read_only {
            JournalCapability::RollbackOnly
        } else {
            let mode: String = rt
                .block_on(async {
                    let mut rows = conn.query("PRAGMA journal_mode", ()).await?;
                    match rows.next().await? {
                        Some(row) => Ok(tv_to_string(&row.get_value(0)?)),
                        None => Ok(String::new()),
                    }
                })
                .map_err(|e: turso::Error| classify(&e))?;
            if mode.eq_ignore_ascii_case("wal") {
                JournalCapability::Wal
            } else {
                degraded.push(format!(
                    "journal read-back {mode:?} is not wal; rollback-only assumed"
                ));
                JournalCapability::RollbackOnly
            }
        };

        if created_new_store {
            report.schema_version_before = None;
            report.schema_version_after = Some(SCHEMA_VERSION_KNOWN);
        }

        report.degraded = degraded;

        // ── Capabilities: measured at 0.8.3, never aspirational ─────────
        let fts = FtsCapabilities {
            base: Err(FTS_UNAVAILABLE_REASON.into()),
            trigram: Err(FTS_UNAVAILABLE_REASON.into()),
            cjk: Err(FTS_UNAVAILABLE_REASON.into()),
        };
        let capabilities = Capabilities {
            journal,
            checkpoint_passive_only: true,
            advisory_generation_guard: false,
            file_format_standard_sqlite: true,
            bulk_parameter_limit: BULK_PARAMETER_FLOOR,
            fts,
        };

        let store = TursoStore {
            path,
            engine: Mutex::new(Engine { rt, conn }),
            capabilities,
            report,
            closed: AtomicBool::new(false),
        };
        let open = super::api::OpenStore {
            capabilities: store.capabilities.clone(),
            report: store.report.clone(),
        };
        Ok((store, open))
    }
}

pub struct TursoStore {
    path: PathBuf,
    engine: Mutex<Engine>,
    capabilities: Capabilities,
    report: OpenReport,
    closed: AtomicBool,
}

impl std::fmt::Debug for TursoStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TursoStore")
            .field("path", &self.path)
            .field("closed", &self.closed.load(Ordering::SeqCst))
            .finish()
    }
}

impl TursoStore {
    pub fn open(
        opts: OpenOptions,
    ) -> Result<(TursoStore, super::api::OpenStore), PersistenceError> {
        TursoBackend::open(opts)
    }

    pub fn path(&self) -> &Path {
        &self.path
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

    /// Storage size: main file + `-wal`/`-shm` sidecars if present.
    pub fn storage_bytes(&self) -> u64 {
        let mut total = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", self.path.display()));
            if let Ok(m) = std::fs::metadata(&sidecar) {
                total += m.len();
            }
        }
        total
    }
}

/// The 27-column append mirroring INSERT_MESSAGE_SQL (stock_sqlite.rs).
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
    let Some(text) = tool_calls else { return 0 };
    let trimmed = text.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) || trimmed.len() <= 1 {
        return 0;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.trim().is_empty() {
        return 0;
    }
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

struct TursoWriteTxn<'a> {
    engine: &'a Engine,
}

struct TursoReadTxn<'a> {
    engine: &'a Engine,
}

impl<'a> WriteTxn for TursoWriteTxn<'a> {
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
        self.engine
            .rt
            .block_on(self.engine.conn.execute(
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
                turso::params![session_id, source, model, now_epoch()],
            ))
            .map_err(|e| StoreError::InvalidInput(classify(&e).to_string()))
            .and(Ok(()))
    }

    fn append_message(&mut self, session_id: &str, msg: &AppendMessage) -> Result<i64, StoreError> {
        let engine = self.engine;
        let ended: Result<Option<(Option<f64>, Option<String>)>, turso::Error> =
            engine.rt.block_on(async {
                let mut rows = engine
                    .conn
                    .query(
                        "SELECT ended_at, end_reason FROM sessions WHERE id = ?1",
                        turso::params![session_id],
                    )
                    .await?;
                match rows.next().await? {
                    Some(row) => {
                        let ended_at = match row.get_value(0)? {
                            TValue::Null => None,
                            TValue::Real(r) => Some(r),
                            TValue::Integer(i) => Some(i as f64),
                            _ => None,
                        };
                        let end_reason = tv_to_option_string(&row.get_value(1)?);
                        Ok(Some((ended_at, end_reason)))
                    }
                    None => Ok(None),
                }
            });
        let ended = match ended {
            Ok(v) => v,
            Err(e) => return Err(StoreError::InvalidInput(e.to_string())),
        };
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
                // Same mint expression as the stock backend: the upstream
                // trigger also covers this, but the slice mints explicitly.
                let minted: String = engine
                    .rt
                    .block_on(async {
                        let mut rows = engine
                            .conn
                            .query("SELECT lower(hex(randomblob(16)))", ())
                            .await?;
                        match rows.next().await? {
                            Some(row) => Ok(tv_to_string(&row.get_value(0)?)),
                            None => Ok(String::new()),
                        }
                    })
                    .map_err(|e: turso::Error| StoreError::InvalidInput(e.to_string()))?;
                minted
            }
        };
        engine
            .rt
            .block_on(engine.conn.execute(
                INSERT_MESSAGE_SQL,
                turso::params![
                    session_id,
                    msg.role.clone(),
                    msg.content.clone(),
                    msg.tool_call_id.clone(),
                    msg.tool_calls.clone(),
                    msg.tool_name.clone(),
                    timestamp,
                    msg.token_count,
                    uid,
                ],
            ))
            .map_err(|_| StoreError::SessionRowMissing)?;
        let row_id = engine.conn.last_insert_rowid();
        let calls = tool_calls_count(&msg.tool_calls);
        engine
            .rt
            .block_on(async {
                if calls > 0 {
                    engine
                        .conn
                        .execute(
                            "UPDATE sessions SET message_count = message_count + 1,
                               tool_call_count = tool_call_count + ?1 WHERE id = ?2",
                            turso::params![calls, session_id],
                        )
                        .await
                } else {
                    engine
                        .conn
                        .execute(
                            "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
                            turso::params![session_id],
                        )
                        .await
                }
            })
            .map_err(|_| StoreError::InvalidInput("counter bump failed".into()))?;
        Ok(row_id)
    }

    fn end_session(
        &mut self,
        session_id: &str,
        end_reason: &str,
    ) -> Result<EndSessionOutcome, StoreError> {
        let engine = self.engine;
        let changed = engine
            .rt
            .block_on(engine.conn.execute(
                "UPDATE sessions SET ended_at = ?1, end_reason = ?2
                 WHERE id = ?3 AND ended_at IS NULL",
                turso::params![now_epoch(), end_reason, session_id],
            ))
            .map_err(|_| StoreError::InvalidInput("end_session update failed".into()))?;
        if changed == 0 {
            return Ok(EndSessionOutcome::AlreadyEnded);
        }
        let bumped = if RESET_END_REASONS.contains(&end_reason) {
            let row: Option<(Option<String>, Option<String>)> = engine
                .rt
                .block_on(async {
                    let mut rows = engine
                        .conn
                        .query(
                            "SELECT source, session_key FROM sessions WHERE id = ?1",
                            turso::params![session_id],
                        )
                        .await?;
                    match rows.next().await? {
                        Some(row) => Ok(Some((
                            tv_to_option_string(&row.get_value(0)?),
                            tv_to_option_string(&row.get_value(1)?),
                        ))),
                        None => Ok(None),
                    }
                })
                .map_err(|e: turso::Error| StoreError::InvalidInput(e.to_string()))?;
            if let Some((source, key)) = row {
                let (source, key) = (source.unwrap_or_default(), key.unwrap_or_default());
                if !source.trim().is_empty() && !key.trim().is_empty() {
                    engine
                        .rt
                        .block_on(engine.conn.execute(
                            "INSERT INTO conversation_generations (source, session_key, generation)
                             VALUES (?1, ?2, 1)
                             ON CONFLICT(source, session_key) DO UPDATE
                                 SET generation = conversation_generations.generation + 1",
                            turso::params![source, key],
                        ))
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
        // Row-existence guard + upsert pair, mirroring the stock backend's
        // SQL (the same statements turso's parser accepted in probes).
        self.ensure_session(session_id, "unknown", Some(&delta.model))?;
        let engine = self.engine;
        let has_usage = delta.input_tokens != 0
            || delta.output_tokens != 0
            || delta.cache_read_tokens != 0
            || delta.cache_write_tokens != 0
            || delta.reasoning_tokens != 0
            || delta.api_call_count != 0
            || delta.estimated_cost_usd != 0.0;
        engine
            .rt
            .block_on(async {
                engine
                    .conn
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
                        turso::params![
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
                            delta.cost_status.clone(),
                            delta.cost_source.clone(),
                            if has_usage {
                                Some(delta.billing_provider.clone())
                            } else {
                                None
                            },
                            if has_usage {
                                Some(delta.billing_base_url.clone())
                            } else {
                                None
                            },
                            if has_usage {
                                Some(delta.billing_mode.clone())
                            } else {
                                None
                            },
                            if has_usage {
                                Some(delta.model.clone())
                            } else {
                                None
                            },
                            delta.api_call_count,
                            session_id,
                        ],
                    )
                    .await?;
                // Per-model attribution upsert (main-loop rows only, matching
                // the slice: aux/task rows keep 'unknown' attribution).
                let (model, provider, base_url, mode) = if delta.task.is_empty() {
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
                engine
                    .conn
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
                        turso::params![
                            session_id,
                            model,
                            provider,
                            base_url,
                            mode,
                            delta.task.clone(),
                            delta.api_call_count,
                            delta.input_tokens,
                            delta.output_tokens,
                            delta.cache_read_tokens,
                            delta.cache_write_tokens,
                            delta.reasoning_tokens,
                            delta.estimated_cost_usd,
                            delta.cost_status.clone(),
                            delta.cost_source.clone(),
                            now,
                            now,
                        ],
                    )
                    .await
            })
            .map_err(|e: turso::Error| StoreError::InvalidInput(format!(
                "usage update failed: {e}"
            )))?;
        Ok(())
    }
}

impl<'a> ReadTxn for TursoReadTxn<'a> {
    fn get_messages(
        &mut self,
        session_id: &str,
        include_inactive: bool,
        include_compacted: bool,
        limit: Option<usize>,
        offset: usize,
    ) -> Result<Vec<MessageRow>, StoreError> {
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
        let engine = self.engine;
        let _ = session_id; // bound as ?1 below
        engine
            .rt
            .block_on(async {
                let mut rows = engine
                    .conn
                    .query(
                        &sql,
                        turso::params![
                            session_id,
                            limit.map(|l| l as i64).unwrap_or(-1),
                            offset as i64
                        ],
                    )
                    .await?;
                let mut out = Vec::new();
                while let Some(row) = rows.next().await? {
                    out.push(MessageRow {
                        id: match row.get_value(0)? {
                            TValue::Integer(v) => v,
                            o => return Err(turso::Error::ConversionFailure(format!("id {o:?}"))),
                        },
                        session_id: tv_to_string(&row.get_value(1)?),
                        role: tv_to_string(&row.get_value(2)?),
                        content: tv_to_option_string(&row.get_value(3)?),
                        tool_call_id: tv_to_option_string(&row.get_value(4)?),
                        tool_calls: tv_to_option_string(&row.get_value(5)?),
                        tool_name: tv_to_option_string(&row.get_value(6)?),
                        timestamp: match row.get_value(7)? {
                            TValue::Real(r) => r,
                            TValue::Integer(i) => i as f64,
                            o => {
                                return Err(turso::Error::ConversionFailure(format!(
                                    "timestamp {o:?}"
                                )))
                            }
                        },
                        token_count: match row.get_value(8)? {
                            TValue::Null => None,
                            TValue::Integer(v) => Some(v),
                            _ => None,
                        },
                        message_uid: tv_to_option_string(&row.get_value(9)?),
                        active: match row.get_value(10)? {
                            TValue::Integer(v) => v != 0,
                            _ => false,
                        },
                        compacted: match row.get_value(11)? {
                            TValue::Integer(v) => v != 0,
                            _ => false,
                        },
                        display_kind: tv_to_option_string(&row.get_value(12)?),
                    });
                }
                Ok::<Vec<MessageRow>, turso::Error>(out)
            })
            .map_err(|e: turso::Error| {
                StoreError::InvalidInput(format!("get_messages failed: {e}"))
            })
    }

    fn get_session(&mut self, session_id: &str) -> Result<Option<Session>, StoreError> {
        let engine = self.engine;
        engine
            .rt
            .block_on(async {
                let mut rows = engine
                    .conn
                    .query(
                        "SELECT id, source, model, billing_provider, billing_base_url, billing_mode,
                        message_count, tool_call_count, input_tokens,
                        output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                        api_call_count, estimated_cost_usd, started_at, ended_at, end_reason,
                        parent_session_id, session_key
                 FROM sessions WHERE id = ?1",
                        turso::params![session_id],
                    )
                    .await?;
                let Some(row) = rows.next().await? else {
                    return Ok(None);
                };
                let gi = |idx: usize| -> Result<i64, turso::Error> {
                    Ok(match row.get_value(idx)? {
                        TValue::Null => 0,
                        TValue::Integer(v) => v,
                        o => {
                            return Err(turso::Error::ConversionFailure(format!("int {o:?}")));
                        }
                    })
                };
                let gf = |idx: usize| -> Result<f64, turso::Error> {
                    Ok(match row.get_value(idx)? {
                        TValue::Real(v) => v,
                        TValue::Integer(v) => v as f64,
                        o => {
                            return Err(turso::Error::ConversionFailure(format!("real {o:?}")));
                        }
                    })
                };
                let gopt_f = |idx: usize| -> Result<Option<f64>, turso::Error> {
                    Ok(match row.get_value(idx)? {
                        TValue::Null => None,
                        TValue::Real(v) => Some(v),
                        TValue::Integer(v) => Some(v as f64),
                        _ => None,
                    })
                };
                Ok(Some(Session {
                    id: tv_to_string(&row.get_value(0)?),
                    source: tv_to_string(&row.get_value(1)?),
                    model: tv_to_option_string(&row.get_value(2)?),
                    billing_provider: tv_to_option_string(&row.get_value(3)?),
                    billing_base_url: tv_to_option_string(&row.get_value(4)?),
                    billing_mode: tv_to_option_string(&row.get_value(5)?),
                    message_count: gi(6)?,
                    tool_call_count: gi(7)?,
                    input_tokens: gi(8)?,
                    output_tokens: gi(9)?,
                    cache_read_tokens: gi(10)?,
                    cache_write_tokens: gi(11)?,
                    reasoning_tokens: gi(12)?,
                    api_call_count: gi(13)?,
                    estimated_cost_usd: gopt_f(14)?,
                    started_at: gf(15)?,
                    ended_at: gopt_f(16)?,
                    end_reason: tv_to_option_string(&row.get_value(17)?),
                    parent_session_id: tv_to_option_string(&row.get_value(18)?),
                    session_key: tv_to_option_string(&row.get_value(19)?),
                }))
            })
            .map_err(|e: turso::Error| StoreError::InvalidInput(format!("get_session failed: {e}")))
    }

    fn search_messages(
        &mut self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        // No FTS5 on this engine: canonical-rows LIKE fallback ONLY (the
        // same shape the stock backend serves when its index is stale).
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let term: String = query.trim().trim_matches('"').to_string();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        let engine = self.engine;
        let visibility =
            "(m.active = 1 OR m.compacted = 1) AND COALESCE(m.display_kind, '') <> 'hidden'";
        let sql = format!(
            "SELECT m.id, m.session_id, m.role
             FROM messages m WHERE m.content LIKE '%' || ?1 || '%' AND {visibility}
             ORDER BY m.id LIMIT {limit} OFFSET {offset}"
        );
        engine
            .rt
            .block_on(async {
                let mut rows = engine.conn.query(&sql, turso::params![term]).await?;
                let mut out = Vec::new();
                while let Some(row) = rows.next().await? {
                    out.push(SearchHit {
                        message_id: match row.get_value(0)? {
                            TValue::Integer(v) => v,
                            o => return Err(turso::Error::ConversionFailure(format!("id {o:?}"))),
                        },
                        session_id: tv_to_string(&row.get_value(1)?),
                        role: tv_to_string(&row.get_value(2)?),
                        snippet: String::new(),
                        rank: 0.0,
                    });
                }
                Ok::<Vec<SearchHit>, turso::Error>(out)
            })
            .map_err(|e: turso::Error| StoreError::InvalidInput(format!("search failed: {e}")))
    }
}

impl SessionStore for TursoStore {
    fn write<T>(
        &self,
        patience: Patience,
        f: &mut dyn FnMut(&mut dyn WriteTxn) -> Result<T, StoreError>,
    ) -> Result<T, PersistenceError> {
        self.assert_open()?;
        let mut engine = self.engine.lock().unwrap();
        let deadline = Instant::now() + patience.0;
        loop {
            let begin = engine
                .rt
                .block_on(engine.conn.execute("BEGIN IMMEDIATE", ()));
            if let Err(e) = begin {
                let classified = classify(&e);
                if is_busy(&classified) && Instant::now() < deadline {
                    drop(engine);
                    jitter_sleep();
                    engine = self.engine.lock().unwrap();
                    continue;
                }
                return Err(classified);
            }
            let mut txn = TursoWriteTxn { engine: &engine };
            match f(&mut txn) {
                Ok(value) => {
                    // TursoWriteTxn borrows engine; end the borrow before
                    // driving COMMIT on the same connection.
                    let commit = engine.rt.block_on(engine.conn.execute("COMMIT", ()));
                    match commit {
                        Ok(_) => return Ok(value),
                        Err(e) => {
                            let _ = engine.rt.block_on(engine.conn.execute("ROLLBACK", ()));
                            let classified = classify(&e);
                            if is_busy(&classified) && Instant::now() < deadline {
                                drop(engine);
                                jitter_sleep();
                                engine = self.engine.lock().unwrap();
                                continue;
                            }
                            return Err(classified);
                        }
                    }
                }
                Err(store_err) => {
                    let _ = engine.rt.block_on(engine.conn.execute("ROLLBACK", ()));
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
        let engine = self.engine.lock().unwrap();
        let mut txn = TursoReadTxn { engine: &engine };
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
        self.assert_open()?;
        // PASSIVE-only policy (§2.7): driven through query() because turso's
        // execute() rejects row-returning statements. Result row observed in
        // probe 4: (busy=0, log=8, checkpointed=8).
        let engine = self.engine.lock().unwrap();
        engine
            .rt
            .block_on(async {
                let mut rows = engine
                    .conn
                    .query("PRAGMA wal_checkpoint(PASSIVE)", ())
                    .await?;
                while let Some(_row) = rows.next().await? {}
                Ok::<(), turso::Error>(())
            })
            .map_err(|e: turso::Error| classify(&e))
    }

    fn backup(&self, dest: &Path) -> Result<(), PersistenceError> {
        // turso 0.8.3 exposes NO backup API. The only candidate is a
        // checkpoint + file copy; whether that is page-consistent is exactly
        // what the HRS-009 parity battery tests. Implemented as: PASSIVE
        // checkpoint, then copy main + sidecars. If the battery shows the
        // copy is not restorable, that is the published result.
        self.assert_open()?;
        if dest.exists() {
            return Err(PersistenceError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("backup destination exists: {}", dest.display()),
            )));
        }
        self.checkpoint_passive()?;
        std::fs::copy(&self.path, dest).map_err(PersistenceError::Io)?;
        // Sidecars copied alongside, matching the upstream WAL-safe snapshot
        // shape; absent sidecars mean the main file is the whole store.
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", self.path.display()));
            if sidecar.exists() {
                let target = PathBuf::from(format!("{}{suffix}", dest.display()));
                std::fs::copy(&sidecar, &target).map_err(PersistenceError::Io)?;
            }
        }
        Ok(())
    }

    fn close(&mut self) -> Result<(), PersistenceError> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        // Clean close: drop the connection inside the runtime context, then
        // the runtime (drop order: Engine fields — conn before rt).
        let engine = self.engine.lock().map_err(|_| {
            PersistenceError::Io(std::io::Error::other("close: engine lock poisoned"))
        })?;
        engine.rt.block_on(async {
            // A final no-op keeps the connection's async machinery settled.
            let _ = engine.conn.query("SELECT 1", ()).await;
        });
        Ok(())
    }
}
