//! DF-HRS-114: the adapter's boolean `open` parameters are typed, not
//! coerced (specs/python-interop.md §4.1).
//!
//! Every case here drives a REAL adapter subprocess over stdio
//! (newline-delimited JSON-RPC, the house `hrs003_python_interop.rs`
//! pattern) and asserts the documented outcome for `create`/`read_only`
//! given as boolean, string, number, and null:
//!
//! - a wrong type is refused with `InvalidInput` naming the parameter and
//!   the accepted type, and NEVER silently changes the open mode: no store
//!   is created for `create: "yes"`, and `read_only: "true"` cannot yield a
//!   write-capable handle on an existing archive (file bytes unchanged);
//! - absent/`null` keeps the documented default (both false);
//! - the correct boolean still works exactly as before.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn adapter_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hermes-rs-session-adapter"))
}

/// One adapter process per case (the adapter serves one store per process):
/// write frames, close stdin, return the reply frames (one JSON per line).
fn exchange(frames: &[Value]) -> Vec<Value> {
    let mut cmd = Command::new(adapter_path());
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn adapter binary");
    {
        let stdin = child.stdin.as_mut().expect("stdin piped");
        for f in frames {
            stdin
                .write_all(format!("{f}\n").as_bytes())
                .expect("write frame");
        }
    }
    let out = child.wait_with_output().expect("collect adapter output");
    assert!(
        out.stderr.is_empty(),
        "adapter wrote unexpected stderr: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).expect("each response line is one JSON frame"))
        .collect()
}

/// Open frames for `path` with `create`/`read_only` as the given JSON values.
fn open_params(path: &str, create: Value, read_only: Value) -> Value {
    json!({
        "id": 1,
        "method": "open",
        "params": {"path": path, "create": create, "read_only": read_only}
    })
}

/// An `InvalidInput` reply naming the parameter and the accepted type.
fn expect_wrong_type_replied(responses: &[Value], key: &str, got: &str) {
    assert_eq!(responses.len(), 1, "one reply per open frame");
    let err = &responses[0]["error"];
    assert_eq!(
        err["error"].as_str(),
        Some("InvalidInput"),
        "wrong-typed {key} must be refused, reply was: {responses:?}"
    );
    assert_eq!(
        err["detail"].as_str(),
        Some(format!("params.{key} must be a JSON boolean; got {got}").as_str()),
        "error must name the parameter and the accepted type"
    );
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hrs114-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

/// Build a real store file with the library (independent of the adapter),
/// then return its bytes so each exchange can prove byte-identity.
fn seed_store(dir: &std::path::Path) -> PathBuf {
    use hermes_rs::persistence::{
        OpenOptions, Patience, SessionStore, StockSqliteBackend, WriteTxn,
    };
    let path = dir.join("existing.db");
    let mut store = StockSqliteBackend::open(OpenOptions::read_write(path.clone()))
        .expect("seed store open")
        .0;
    store
        .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
            tx.ensure_session("hrs114-seed", "test", None)
        })
        .expect("seed session");
    store.close().expect("seed close");
    path
}

fn file_bytes(path: &std::path::Path) -> Vec<u8> {
    std::fs::read(path).expect("read store file")
}

#[test]
fn hrs114_wrong_typed_create_never_creates_a_store() {
    for (tag, create, got) in [
        ("create-string", json!("yes"), "string"),
        ("create-number", json!(1), "number"),
    ] {
        let dir = temp_dir(tag);
        let db = dir.join("never-created.db");
        let responses = exchange(&[
            open_params(db.to_str().unwrap(), create, json!(false)),
            json!({"id": 2, "method": "shutdown"}),
        ]);
        expect_wrong_type_replied(&responses, "create", got);
        assert!(
            !db.exists(),
            "a wrong-typed create must not create the store at {db:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn hrs114_null_and_absent_create_keep_the_default_false() {
    // Explicit null: not a wrong type, defaults to false. On a nonexistent
    // path a non-creating open fails with the store's Io error (the
    // documented default path) — it must NOT be an InvalidInput.
    let dir = temp_dir("create-null");
    let db = dir.join("absent-default.db");
    let responses = exchange(&[
        open_params(db.to_str().unwrap(), Value::Null, json!(false)),
        json!({"id": 2, "method": "shutdown"}),
    ]);
    assert_eq!(responses.len(), 1);
    let err = &responses[0]["error"];
    assert_eq!(
        err["error"].as_str(),
        Some("Io"),
        "null create must keep the default (no create), reply was: {responses:?}"
    );
    assert!(!db.exists());
    let _ = std::fs::remove_dir_all(&dir);

    // Absent field: identical documented default.
    let dir = temp_dir("create-absent");
    let db = dir.join("absent-default.db");
    let responses = exchange(&[
        json!({"id": 1, "method": "open", "params": {"path": db.to_str().unwrap()}}),
        json!({"id": 2, "method": "shutdown"}),
    ]);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["error"].as_str(), Some("Io"));
    assert!(!db.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hrs114_boolean_create_still_works_exactly_as_before() {
    let dir = temp_dir("create-true");
    let db = dir.join("created.db");
    let responses = exchange(&[
        open_params(db.to_str().unwrap(), json!(true), json!(false)),
        json!({"id": 2, "method": "shutdown"}),
    ]);
    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    assert!(
        result.is_object(),
        "create: true must open successfully, reply was: {responses:?}"
    );
    assert_eq!(result["report"]["created_new_store"], json!(true));
    assert!(db.exists(), "create: true must create the store");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hrs114_wrong_typed_read_only_never_yields_a_write_handle() {
    // The finding's exact scenario: read_only as a STRING together with
    // create: true used to be treated as false — a caller believing they
    // opened a read-only viewer got a write-capable handle and the file was
    // created on disk. Both must now be refused and nothing created.
    let dir = temp_dir("ro-string-fresh");
    let db = dir.join("archive.db");
    let responses = exchange(&[
        open_params(db.to_str().unwrap(), json!(true), json!("true")),
        json!({"id": 2, "method": "shutdown"}),
    ]);
    expect_wrong_type_replied(&responses, "read_only", "string");
    assert!(
        !db.exists(),
        "read_only: \"true\" must not fall back to a creating write open at {db:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);

    // On an EXISTING archive the wrong-typed read_only must be refused
    // without opening anything: the file stays byte-identical (no
    // write-capable handle ever touched it).
    let dir = temp_dir("ro-string-existing");
    let db = seed_store(&dir);
    let before = file_bytes(&db);
    let responses = exchange(&[
        open_params(db.to_str().unwrap(), json!(false), json!("true")),
        json!({"id": 2, "method": "shutdown"}),
    ]);
    expect_wrong_type_replied(&responses, "read_only", "string");
    assert_eq!(
        file_bytes(&db),
        before,
        "a refused read_only: \"true\" open must leave the archive byte-identical"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hrs114_read_only_boolean_and_null_keep_documented_behavior() {
    let dir = temp_dir("ro-bool-null");
    let db = seed_store(&dir);
    let before = file_bytes(&db);

    // read_only: true — the real read-only path: opens, file byte-identical.
    let responses = exchange(&[
        open_params(db.to_str().unwrap(), json!(false), json!(true)),
        json!({"id": 2, "method": "shutdown"}),
    ]);
    assert_eq!(responses.len(), 1);
    assert!(
        responses[0]["result"].is_object(),
        "read_only: true must open successfully, reply was: {responses:?}"
    );
    assert_eq!(
        responses[0]["result"]["report"]["created_new_store"],
        json!(false)
    );
    assert_eq!(
        file_bytes(&db),
        before,
        "a boolean read-only open must leave the archive byte-identical"
    );

    // read_only: null — not a wrong type, defaults to false (a write-mode
    // open of the existing store succeeds).
    let responses = exchange(&[
        open_params(db.to_str().unwrap(), json!(false), Value::Null),
        json!({"id": 2, "method": "shutdown"}),
    ]);
    assert_eq!(responses.len(), 1);
    assert!(
        responses[0]["result"].is_object(),
        "null read_only must keep the default (write mode), reply was: {responses:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
