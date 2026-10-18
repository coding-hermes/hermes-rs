//! DF-HRS-106: `OpenReport.created_new_store` must reflect what open()
//! actually did — `true` only on the open that scaffolded a fresh store,
//! `false` on every reopen of an existing one.
//!
//! Regression context: the local `created_new_store` flag was computed in
//! both backends' open() (fresh-scaffold branch) but never copied into the
//! returned `OpenReport`, so `open(create=true)` on a fresh path reported
//! `created_new_store: false` even though the store WAS created.
//!
//! The stock arm runs unconditionally so this battery cannot rot (same
//! convention as the HRS-009 parity suite); the turso arm is feature-gated
//! because the engine is an optional dependency.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use hermes_rs::persistence::{OpenOptions, StockSqliteBackend};

#[cfg(feature = "turso")]
use hermes_rs::persistence::TursoBackend;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "hrs106-{}-{}-{tag}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.db")
}

#[test]
fn hrs106_created_new_store_fresh_then_reopen_stock() {
    let path = scratch("stock");
    assert!(!path.exists(), "precondition: fresh path");

    // Fresh scaffold: the store did not exist, open(create=true) created it.
    let (store, open) =
        StockSqliteBackend::open(OpenOptions::read_write(&path)).expect("fresh scaffold open");
    assert!(
        open.report.created_new_store,
        "first open on a fresh path must report created_new_store: true"
    );
    assert!(
        open.report.schema_version_before.is_none(),
        "a created store has no prior schema version"
    );
    drop(store);

    // Reopen: the store exists now, nothing is created.
    let (_store, open) =
        StockSqliteBackend::open(OpenOptions::read_write(&path)).expect("reopen open");
    assert!(
        !open.report.created_new_store,
        "reopen of an existing store must report created_new_store: false"
    );
    assert!(
        open.report.schema_version_before.is_some(),
        "reopen sees the prior schema version"
    );

    // Read-only open of the same store: recognition never creates.
    let (_store, open) =
        StockSqliteBackend::open(OpenOptions::read_only(&path)).expect("read-only open");
    assert!(
        !open.report.created_new_store,
        "read-only open never creates"
    );
}

#[cfg(feature = "turso")]
#[test]
fn hrs106_created_new_store_fresh_then_reopen_turso() {
    let path = scratch("turso");
    assert!(!path.exists(), "precondition: fresh path");

    let (store, open) =
        TursoBackend::open(OpenOptions::read_write(&path)).expect("fresh scaffold open");
    assert!(
        open.report.created_new_store,
        "turso: first open on a fresh path must report created_new_store: true"
    );
    drop(store);

    let (_store, open) = TursoBackend::open(OpenOptions::read_write(&path)).expect("reopen open");
    assert!(
        !open.report.created_new_store,
        "turso: reopen of an existing store must report created_new_store: false"
    );
}
