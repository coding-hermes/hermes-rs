//! DF-HRS-107: a FRESH store must open with `OpenReport.degraded == []`.
//!
//! Regression context: upstream creates `idx_messages_platform_msg_id`
//! (hermes_state_schema.py:958, imperative, immediately before
//! `DEFERRED_INDEX_SQL` in the same startup block) and
//! `idx_sessions_title_unique` (hermes_state_schema.py:135, executed via
//! `_ensure_unique_title_index` at :1002) OUTSIDE any embedded DDL string, so
//! the verbatim-DDL fresh scaffold never created them while
//! FRESH_STORE_CENSUS_JSON (captured from a real fresh upstream store) lists
//! them — every fresh open reported
//! `degraded: ["fresh-store census mismatch after scaffold"]`.
//! Fix: both CREATE INDEX statements live at the end of
//! UPSTREAM_DEFERRED_INDEX_SQL, which the fresh scaffold executes (and every
//! writable reopen re-runs idempotently, matching upstream's own ordering).
//!
//! The stock arm runs unconditionally so this battery cannot rot (same
//! convention as the HRS-106 suite); there is no turso arm because the census
//! gate is stock-only (turso cannot host FTS at 0.8.3 and is documented as
//! census-unequal by design, specs/backend-evaluation.md).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use hermes_rs::persistence::{OpenOptions, StockSqliteBackend};
use rusqlite::Connection;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "hrs107-{}-{}-{tag}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.db")
}

fn sqlite_master_has_index(path: &PathBuf, name: &str) -> bool {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1")
        .unwrap();
    stmt.exists([name]).unwrap()
}

#[test]
fn hrs107_fresh_store_opens_with_empty_degraded_stock() {
    let path = scratch("stock");
    assert!(!path.exists(), "precondition: fresh path");

    let (store, open) =
        StockSqliteBackend::open(OpenOptions::read_write(&path)).expect("fresh scaffold open");
    assert!(
        open.report.created_new_store,
        "precondition: first open on a fresh path scaffolds the store"
    );
    assert!(
        open.report.degraded.is_empty(),
        "fresh open must not report degraded signals, got: {:?}",
        open.report.degraded
    );
    drop(store);

    // Independent of the census gate: the two upstream-imperative indexes must
    // physically exist in the fresh file.
    assert!(
        sqlite_master_has_index(&path, "idx_sessions_title_unique"),
        "fresh store is missing idx_sessions_title_unique"
    );
    assert!(
        sqlite_master_has_index(&path, "idx_messages_platform_msg_id"),
        "fresh store is missing idx_messages_platform_msg_id"
    );
}

#[test]
fn hrs107_fresh_store_reopen_stays_clean_stock() {
    let path = scratch("stock-reopen");
    assert!(!path.exists(), "precondition: fresh path");

    let (store, _open) =
        StockSqliteBackend::open(OpenOptions::read_write(&path)).expect("fresh scaffold open");
    drop(store);

    // Reopen must stay clean AND must not regress the indexes (the deferred
    // index pass re-runs idempotently on every writable open).
    let (_store, open) =
        StockSqliteBackend::open(OpenOptions::read_write(&path)).expect("reopen open");
    assert!(
        !open.report.created_new_store,
        "reopen must not report created_new_store"
    );
    assert!(
        open.report.degraded.is_empty(),
        "reopen must not report degraded signals, got: {:?}",
        open.report.degraded
    );
    assert!(sqlite_master_has_index(&path, "idx_sessions_title_unique"));
    assert!(sqlite_master_has_index(
        &path,
        "idx_messages_platform_msg_id"
    ));
}
