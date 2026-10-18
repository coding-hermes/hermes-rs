// DF-HRS-108: backup() must accept an existing destination (upstream pre-creates).
use hermes_rs::persistence::{OpenOptions, SessionStore, StockSqliteBackend};
use std::path::PathBuf;

fn tmp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("hrs108-{}-{}.db", std::process::id(), label))
}

#[test]
fn backup_to_existing_destination_succeeds() {
    let src_path = tmp_path("src");
    let dest_path = tmp_path("dest");

    let (store, _open) =
        StockSqliteBackend::open(OpenOptions::read_write(&src_path)).expect("open source");

    std::fs::write(&dest_path, b"placeholder").expect("pre-create dest");
    assert!(dest_path.exists(), "dest must exist before backup");

    store.backup(&dest_path).expect("backup to existing path");

    let dest_content = std::fs::read(&dest_path).expect("read dest");
    assert!(
        dest_content.len() > 100,
        "dest should be a real SQLite file, not placeholder"
    );
    assert_eq!(&dest_content[0..6], b"SQLite", "dest must be SQLite");

    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&dest_path);
}

#[test]
fn backup_to_new_path_still_works() {
    let src_path = tmp_path("src2");
    let dest_path = tmp_path("new");

    let (store, _open) =
        StockSqliteBackend::open(OpenOptions::read_write(&src_path)).expect("open source");

    assert!(!dest_path.exists(), "dest must not exist yet");
    store.backup(&dest_path).expect("backup to new path");
    assert!(dest_path.exists(), "dest must exist after backup");

    let dest_content = std::fs::read(&dest_path).expect("read dest");
    assert_eq!(&dest_content[0..6], b"SQLite", "dest must be SQLite");

    let _ = std::fs::remove_file(&src_path);
    let _ = std::fs::remove_file(&dest_path);
}
