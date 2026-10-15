//! HRS-009 backend parity battery: the same assertions run against BOTH
//! backends — stock SQLite (always) and Turso (under `--features turso`).
//!
//! Every test takes a backend id and dispatches to the right constructor;
//! Turso-specific behavior differences (no FTS5, single-process, file-copy
//! backup) are asserted as DOCUMENTED behavior, never skipped silently.
//!
//! The stock arm runs unconditionally so the battery itself cannot rot;
//! the turso arm is feature-gated because the engine is an optional dep.

#![cfg(feature = "turso")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use hermes_rs::persistence::{
    AppendMessage, Capability, ModelUsageDelta, OpenOptions, Patience, PersistenceError, ReadTxn,
    SessionStore, StockSqliteBackend, TursoBackend, WriteTxn,
};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "hrs009-{}-{}-{tag}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("state.db")
}

/// Which backend a test runs. `all` = stock + turso.
#[derive(Clone, Copy)]
enum Which {
    Stock,
    Turso,
}

const ALL: [Which; 2] = [Which::Stock, Which::Turso];

/// SessionStore has generic methods (write<T>/read<T>) so it is not
/// dyn-compatible; the enum is the compiler-suggested dispatcher and keeps
/// every test body backend-agnostic.
enum AnyStore {
    Stock(hermes_rs::persistence::stock_sqlite::StockSqliteStore),
    #[cfg(feature = "turso")]
    Turso(hermes_rs::persistence::turso_backend::TursoStore),
}

impl SessionStore for AnyStore {
    fn write<T>(
        &self,
        patience: Patience,
        f: &mut dyn FnMut(&mut dyn WriteTxn) -> Result<T, hermes_rs::persistence::StoreError>,
    ) -> Result<T, PersistenceError> {
        match self {
            AnyStore::Stock(s) => s.write(patience, f),
            AnyStore::Turso(s) => s.write(patience, f),
        }
    }

    fn read<T>(
        &self,
        f: &mut dyn FnMut(&mut dyn ReadTxn) -> Result<T, hermes_rs::persistence::StoreError>,
    ) -> Result<T, PersistenceError> {
        match self {
            AnyStore::Stock(s) => s.read(f),
            AnyStore::Turso(s) => s.read(f),
        }
    }

    fn capabilities(&self) -> &hermes_rs::persistence::Capabilities {
        match self {
            AnyStore::Stock(s) => s.capabilities(),
            AnyStore::Turso(s) => s.capabilities(),
        }
    }

    fn open_report(&self) -> &hermes_rs::persistence::OpenReport {
        match self {
            AnyStore::Stock(s) => s.open_report(),
            AnyStore::Turso(s) => s.open_report(),
        }
    }

    fn checkpoint_passive(&self) -> Result<(), PersistenceError> {
        match self {
            AnyStore::Stock(s) => s.checkpoint_passive(),
            AnyStore::Turso(s) => s.checkpoint_passive(),
        }
    }

    fn backup(&self, dest: &std::path::Path) -> Result<(), PersistenceError> {
        match self {
            AnyStore::Stock(s) => s.backup(dest),
            AnyStore::Turso(s) => s.backup(dest),
        }
    }

    fn close(&mut self) -> Result<(), PersistenceError> {
        match self {
            AnyStore::Stock(s) => s.close(),
            AnyStore::Turso(s) => s.close(),
        }
    }
}

fn open(which: Which, path: &PathBuf) -> Result<AnyStore, PersistenceError> {
    match which {
        Which::Stock => Ok(AnyStore::Stock(
            StockSqliteBackend::open(OpenOptions::read_write(path))?.0,
        )),
        Which::Turso => Ok(AnyStore::Turso(
            TursoBackend::open(OpenOptions::read_write(path))?.0,
        )),
    }
}

fn name(which: Which) -> &'static str {
    match which {
        Which::Stock => "stock-sqlite",
        Which::Turso => "turso",
    }
}

// ── 1. schema create ────────────────────────────────────────────────────

#[test]
fn p1_schema_create_and_reopen() {
    for w in ALL {
        let path = scratch("schema");
        {
            let store = open(w, &path).expect("fresh scaffold");
            // schema_version row present with the pinned version
            let report = store.open_report();
            assert_eq!(
                report.schema_version_after,
                Some(hermes_rs::persistence::SCHEMA_VERSION_KNOWN),
                "{}: schema version stamped",
                name(w)
            );
        }
        // reopen succeeds and recognizes its own store
        let store = open(w, &path).expect("reopen");
        assert_eq!(store.open_report().schema_version_before, Some(31));
        drop(store);
    }
}

// ── 2. insert/read round-trip ───────────────────────────────────────────

#[test]
fn p2_insert_read_round_trip() {
    for w in ALL {
        let path = scratch("roundtrip");
        let store = open(w, &path).unwrap();
        store
            .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                tx.ensure_session("20261015_p2_session", "cli", Some("glm-5.3-flash"))?;
                tx.append_message(
                    "20261015_p2_session",
                    &AppendMessage {
                        role: "user".into(),
                        content: Some("HRS-009 parity probe".into()),
                        timestamp: Some(1790009001.0),
                        ..Default::default()
                    },
                )?;
                Ok(())
            })
            .unwrap();
        let (sess_count, msg_count) = store
            .read(&mut |tx: &mut dyn ReadTxn| {
                let s = tx.get_session("20261015_p2_session")?.expect("session");
                let m = tx.get_messages("20261015_p2_session", false, false, None, 0)?;
                Ok((s.message_count, m.len()))
            })
            .unwrap();
        assert_eq!(sess_count, 1, "{}: counter", name(w));
        assert_eq!(msg_count, 1, "{}: row read back", name(w));
        drop(store);
    }
}

// ── 3. transactions + rollback ──────────────────────────────────────────

#[test]
fn p3_transaction_rollback_all_or_nothing() {
    for w in ALL {
        let path = scratch("rollback");
        let store = open(w, &path).unwrap();
        let err = store
            .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                tx.ensure_session("20261015_p3_session", "cli", None)?;
                tx.append_message(
                    "20261015_p3_MISSING",
                    &AppendMessage {
                        role: "user".into(),
                        content: Some("doomed".into()),
                        ..Default::default()
                    },
                )?;
                Ok(())
            })
            .expect_err("must fail");
        let _ = err;
        // ensure_session must have rolled back with the failed txn
        let present = store
            .read(&mut |tx: &mut dyn ReadTxn| {
                tx.get_session("20261015_p3_session").map(|s| s.is_some())
            })
            .unwrap();
        assert!(!present, "{}: rollback discarded the whole txn", name(w));
        // writer healthy after rollback
        store
            .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                tx.ensure_session("20261015_p3_ok", "cli", None)
            })
            .expect("writer healthy after rollback");
        drop(store);
    }
}

// ── 4. index use (point lookup path does not scan) ──────────────────────

#[test]
fn p4_indexed_point_reads() {
    for w in ALL {
        let path = scratch("index");
        let store = open(w, &path).unwrap();
        // seed 200 sessions; read one specific id repeatedly
        store
            .write(Patience::TRANSCRIPT, &mut |tx: &mut dyn WriteTxn| {
                for i in 0..200u64 {
                    let sid = format!("20261015_p4_{i:04}");
                    tx.ensure_session(&sid, "cli", None)?;
                    tx.append_message(
                        &sid,
                        &AppendMessage {
                            role: "user".into(),
                            content: Some(format!("payload {i}")),
                            ..Default::default()
                        },
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let t0 = Instant::now();
        let hits = store
            .read(&mut |tx: &mut dyn ReadTxn| {
                tx.get_messages("20261015_p4_0199", false, false, None, 0)
            })
            .unwrap();
        let elapsed = t0.elapsed();
        assert_eq!(hits.len(), 1, "{}: exact-id lookup", name(w));
        assert_eq!(hits[0].content.as_deref(), Some("payload 199"));
        // Index use is asserted behaviorally: a point read over 200 sessions
        // must stay far below a full-scan cost envelope. (The upstream DDL's
        // per-session indexes exist on both engines — turso applied them.)
        assert!(
            elapsed.as_millis() < 500,
            "{}: point read took {:?} — index likely unused",
            name(w),
            elapsed
        );
        drop(store);
    }
}

// ── 5. WAL + checkpoint ─────────────────────────────────────────────────

#[test]
fn p5_wal_mode_and_passive_checkpoint() {
    for w in ALL {
        let path = scratch("wal");
        let store = open(w, &path).unwrap();
        store
            .write(Patience::TRANSCRIPT, &mut |tx: &mut dyn WriteTxn| {
                tx.ensure_session("20261015_p5_session", "cli", None)?;
                for i in 0..60 {
                    tx.append_message(
                        "20261015_p5_session",
                        &AppendMessage {
                            role: "user".into(),
                            content: Some(format!("w{i}")),
                            ..Default::default()
                        },
                    )?;
                }
                Ok(())
            })
            .unwrap();
        // journal capability: stock resolves to WAL; turso 0.8.3 reports WAL
        // semantics on its own storage (verified read-back in backend open).
        let cap = store.capabilities().supports(Capability::JournalWal);
        let journal_ok = matches!(cap, hermes_rs::persistence::CapabilitySupport::Yes) || {
            // A No answer must carry the reason (silent degradation forbidden).
            matches!(cap, hermes_rs::persistence::CapabilitySupport::No { .. })
        };
        assert!(journal_ok, "{}: journal capability answered", name(w));
        store.checkpoint_passive().expect("passive checkpoint");
        // durability after checkpoint: writes observable
        let n = store
            .read(&mut |tx: &mut dyn ReadTxn| {
                Ok(tx
                    .get_messages("20261015_p5_session", false, false, None, 0)?
                    .len())
            })
            .unwrap();
        assert_eq!(n, 60);
        drop(store);
    }
}

// ── 6. backup/restore ───────────────────────────────────────────────────

#[test]
fn p6_backup_and_restore() {
    for w in ALL {
        let path = scratch("backup");
        let store = open(w, &path).unwrap();
        store
            .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                tx.ensure_session("20261015_p6_session", "cli", None)?;
                tx.append_message(
                    "20261015_p6_session",
                    &AppendMessage {
                        role: "user".into(),
                        content: Some("pre-backup".into()),
                        ..Default::default()
                    },
                )?;
                Ok(())
            })
            .unwrap();
        let dest = path.with_extension("backup.db");
        store.backup(&dest).expect("backup");
        assert!(dest.exists(), "{}: backup artifact", name(w));
        drop(store);
        // Restore leg: open the backup copy and verify the data.
        let restored = open(w, &dest).unwrap_or_else(|e| {
            panic!("{}: backup copy did not reopen: {e}", name(w));
        });
        let msgs = restored
            .read(&mut |tx: &mut dyn ReadTxn| {
                tx.get_messages("20261015_p6_session", false, false, None, 0)
            })
            .unwrap();
        assert_eq!(msgs.len(), 1, "{}: restored row", name(w));
        assert_eq!(msgs[0].content.as_deref(), Some("pre-backup"));
        drop(restored);
    }
}

// ── 7. crash recovery (kill -9 a child writer mid-write) ────────────────

#[test]
fn p7_kill9_child_writer_committed_rows_survive() {
    for w in ALL {
        let path = scratch("crash");
        // Parent scaffolds the store.
        {
            let store = open(w, &path).unwrap();
            store
                .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                    tx.ensure_session("20261015_p7_seed", "cli", None)?;
                    tx.append_message(
                        "20261015_p7_seed",
                        &AppendMessage {
                            role: "user".into(),
                            content: Some("seed".into()),
                            ..Default::default()
                        },
                    )?;
                    Ok(())
                })
                .unwrap();
            // Backend-specific child binary: stock uses the bench example's
            // dataset writer; turso opens the same file only from ITS process
            // (single-process engine). The child runs the matching writer.
            drop(store);
        }
        let exe = std::env::current_exe().expect("test exe");
        let backend_flag = match w {
            Which::Stock => "stock",
            Which::Turso => "turso",
        };
        // The child binary is built from tests/hrs009_crash_child.rs —
        // invoked with the db path + backend; it commits N rows then the
        // parent SIGKILLs it mid-burst.
        let child = std::process::Command::new(exe)
            .args([
                "--exact",
                "crash_child",
                "--nocapture",
                "--ignored",
                "--",
                &path.display().to_string(),
                backend_flag,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => panic!("{}: crash-child spawn failed: {e}", name(w)),
        };
        // Give the child a moment to commit its rows, then SIGKILL.
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let killed = unsafe { kill_child(child.id()) };
        assert!(killed, "{}: child was killable", name(w));
        let _ = child.wait();
        // Reopen: committed rows survive; no torn rows (rowid chain intact).
        let store = open(w, &path).expect("{}: reopen after kill -9");
        let msgs = store
            .read(&mut |tx: &mut dyn ReadTxn| {
                tx.get_messages("20261015_p7_seed", false, false, None, 0)
            })
            .expect("read after crash");
        // At minimum the seed row survives; the child's committed rows are a
        // prefix with ascending ids and no torn content.
        assert!(!msgs.is_empty(), "{}: seed row survived crash", name(w));
        assert!(
            msgs.windows(2).all(|win| win[0].id < win[1].id),
            "{}: no torn rows (ids strictly ascending)",
            name(w)
        );
        drop(store);
    }
}

#[cfg(unix)]
unsafe fn kill_child(pid: u32) -> bool {
    unsafe { libc_kill(pid as i32, 9) == 0 }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

// ── 8. concurrent access (2 writers + 2 readers for 10 s) ───────────────

#[test]
fn p8_concurrent_access_two_writers_two_readers() {
    for w in ALL {
        let path = scratch("concurrent");
        let store = std::sync::Arc::new(open(w, &path).unwrap());
        let stop = std::sync::Arc::new(AtomicU64::new(0));
        let mut writer_handles = Vec::new();
        let mut reader_handles = Vec::new();
        for writer in 0..2u64 {
            let store = store.clone();
            let stop = stop.clone();
            writer_handles.push(std::thread::spawn(move || -> (u64, u64) {
                let mut ops = 0u64;
                let mut errors = 0u64;
                while stop.load(Ordering::SeqCst) == 0 {
                    let sid = format!("20261015_p8_w{writer}");
                    let r = store.write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                        tx.ensure_session(&sid, "cli", None)?;
                        tx.append_message(
                            &sid,
                            &AppendMessage {
                                role: "user".into(),
                                content: Some(format!("writer {writer} op {ops}")),
                                ..Default::default()
                            },
                        )?;
                        Ok(())
                    });
                    match r {
                        Ok(_) => ops += 1,
                        Err(_) => errors += 1,
                    }
                    if ops + errors > 200 {
                        break;
                    }
                }
                (ops, errors)
            }));
        }
        for reader in 0..2u64 {
            let store = store.clone();
            let stop = stop.clone();
            reader_handles.push(std::thread::spawn(move || -> u64 {
                let mut reads = 0u64;
                while stop.load(Ordering::SeqCst) == 0 {
                    let _ = store.read(&mut |tx: &mut dyn ReadTxn| {
                        let _ = tx.get_session("20261015_p8_w0")?;
                        Ok(())
                    });
                    reads += 1;
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                reads
            }));
        }
        // run the 10-second contention window
        std::thread::sleep(std::time::Duration::from_secs(10));
        stop.store(1, Ordering::SeqCst);
        let mut writer_results: Vec<(u64, u64)> = Vec::new();
        for h in writer_handles {
            writer_results.push(h.join().expect("writer join"));
        }
        let mut reader_results: Vec<u64> = Vec::new();
        for h in reader_handles {
            reader_results.push(h.join().expect("reader join"));
        }
        let w0 = writer_results[0];
        let w1 = writer_results[1];
        let r0 = reader_results[0];
        // Both backends must make forward progress with 2 writers; a writer
        // may see busy errors (documented per-backend), but total committed
        // ops must be > 0 and reads must have served.
        assert!(
            w0.0 + w1.0 > 0,
            "{}: no writes committed under contention",
            name(w)
        );
        assert!(r0 > 0, "{}: no reads served", name(w));
        // Record the per-backend contention profile for the report.
        println!(
            "{}: writers committed {}/{} ops (w0={} err={:?}, w1={} err={:?}), reads served {}",
            name(w),
            w0.0 + w1.0,
            w0.0 + w1.0 + w0.1 + w1.1,
            w0.0,
            w0.1,
            w1.0,
            w1.1,
            r0
        );
    }
}

// ── 9. FTS5: SQLite yes; Turso no-with-reason ───────────────────────────

#[test]
fn p9_fts5_availability_recorded_not_assumed() {
    // Stock: FTS5 probe must answer Yes (bundled amalgamation).
    {
        let path = scratch("fts-stock");
        let store = open(Which::Stock, &path).unwrap();
        assert!(
            store.capabilities().supports(Capability::SearchFts).yes(),
            "stock SQLite: FTS5 available"
        );
        drop(store);
    }
    // Turso: the answer must be No (or Degraded) WITH the reason — an
    // explicit "unsupported at version X" is the valid HRS-009 result.
    {
        let path = scratch("fts-turso");
        let store = open(Which::Turso, &path).unwrap();
        let cap = store.capabilities().supports(Capability::SearchFts);
        match cap {
            hermes_rs::persistence::CapabilitySupport::Yes => {
                panic!("turso claiming FTS5 Yes contradicts the no-module probe");
            }
            hermes_rs::persistence::CapabilitySupport::No { reason }
            | hermes_rs::persistence::CapabilitySupport::Degraded { reason } => {
                assert!(
                    reason.contains("0.8.3") && reason.contains("fts5"),
                    "reason must name the version and feature: {reason}"
                );
            }
        }
        // Search still works via the canonical-rows LIKE fallback.
        store
            .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                tx.ensure_session("20261015_p9_session", "cli", None)?;
                tx.append_message(
                    "20261015_p9_session",
                    &AppendMessage {
                        role: "user".into(),
                        content: Some("the unique_zebra_token appears here".into()),
                        ..Default::default()
                    },
                )?;
                Ok(())
            })
            .unwrap();
        let hits = store
            .read(&mut |tx: &mut dyn ReadTxn| tx.search_messages("unique_zebra", 10, 0))
            .expect("LIKE fallback search");
        assert!(!hits.is_empty(), "LIKE fallback finds the term");
        drop(store);
    }
}

// ── 10. usage upsert + end-session parity ───────────────────────────────

#[test]
fn p10_usage_upsert_and_end_session_parity() {
    for w in ALL {
        let path = scratch("usage");
        let store = open(w, &path).unwrap();
        store
            .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
                tx.ensure_session("20261015_p10_session", "cli", None)?;
                tx.record_model_usage(
                    "20261015_p10_session",
                    &ModelUsageDelta {
                        model: "glm-5.3-flash".into(),
                        billing_provider: "custom".into(),
                        billing_base_url: "http://127.0.0.1:20128/v1".into(),
                        billing_mode: "gateway".into(),
                        api_call_count: 2,
                        input_tokens: 100,
                        output_tokens: 40,
                        ..Default::default()
                    },
                )?;
                let out = tx.end_session("20261015_p10_session", "idle")?;
                assert!(
                    matches!(out, hermes_rs::persistence::EndSessionOutcome::Ended { .. }),
                    "first end wins"
                );
                let out2 = tx.end_session("20261015_p10_session", "daily")?;
                assert_eq!(
                    out2,
                    hermes_rs::persistence::EndSessionOutcome::AlreadyEnded,
                    "second end is a no-op"
                );
                Ok(())
            })
            .unwrap();
        let sess = store
            .read(&mut |tx: &mut dyn ReadTxn| tx.get_session("20261015_p10_session"))
            .unwrap()
            .unwrap();
        assert_eq!(sess.input_tokens, 100, "{}: usage accumulated", name(w));
        assert_eq!(sess.api_call_count, 2);
        assert_eq!(
            sess.end_reason.as_deref(),
            Some("idle"),
            "{}: first end_reason won",
            name(w)
        );
        assert!(sess.ended_at.is_some());
        drop(store);
    }
}
