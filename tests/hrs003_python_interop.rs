//! HRS-003 Python interoperability proof: a real CPython 3.11 process (the
//! host interpreter; stdlib `sqlite3`, `json`, `subprocess` only) drives the
//! HRS-010 session store through the process-boundary adapter
//! (`src/bin/session_adapter.rs` + `scripts/hrs003_client.py`).
//!
//! Boundary under test (specs/python-interop.md §5): Python subprocess →
//! newline-delimited JSON-RPC over stdio → Rust session store. Nothing
//! here claims CPython ABI embedding; the proof is the named real package
//! (stdlib sqlite3 as an independent SQLite client) plus the wire protocol.
//!
//! Fail-closed discipline (specs/TEST_ADAPTER_CONTRACT.md §5): the main run
//! is required to succeed; the deliberately broken call is required to FAIL
//! with a structured error — and its transcript is the non-vacuity evidence
//! written to docs/evidence/hrs003/.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

/// The Python-side client paired with this boundary (imported by the proof
/// driver; kept as a constant so renames break this test at compile time).
const CLIENT: &str = "scripts/hrs003_client.py";
const PROOF_DRIVER: &str = "scripts/hrs003_e2e_proof.py";
const BROKEN_TRANSCRIPT: &str = "docs/evidence/hrs003/broken-case-transcript.txt";
const SPEC: &str = "specs/python-interop.md";

/// Locate the Python interpreter: HERMES_RS_PYTHON, else `python3` on PATH.
fn python_bin() -> String {
    std::env::var("HERMES_RS_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = repo root for both the lib and the bin target.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run_python(script: &str, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(python_bin())
        .arg(repo_root().join(script))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("spawn python3 for the interop proof");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn adapter_path() -> PathBuf {
    // Cargo resolves this at build time for the CURRENT target dir
    // (CARGO_TARGET_DIR overrides like this host's shared cache included).
    PathBuf::from(env!("CARGO_BIN_EXE_hermes-rs-session-adapter"))
}

/// The proof run drives the binary through a task-unique copy: the shared
/// CARGO_TARGET_DIR is a clobber surface while sibling workers build
/// (see coding-hermes-worker/shared-cargo-target references), and the
/// Python driver would otherwise race a sibling's rebuild between our
/// build and the subprocess launch.
fn adapter_run_copy(tag: &str) -> PathBuf {
    let src = adapter_path();
    let dest = std::env::temp_dir().join(format!("hrs003-adapter-{tag}-{}", std::process::id()));
    std::fs::copy(&src, &dest).expect("copy adapter binary for the proof run");
    dest
}

// ────────────────────────────────────────────────────────────────────────
// The proof proper: structured round-trip + independent sqlite3 read-back.
// ────────────────────────────────────────────────────────────────────────

#[test]
fn hrs003_python_roundtrip_and_sqlite_readback() {
    assert!(
        CLIENT.len() > 4 && CLIENT.ends_with(".py"),
        "client path constant is a Python file"
    );
    // Task-unique copy: immune to a sibling build clobbering the shared
    // target-dir binary between now and the subprocess launch.
    let run_copy = adapter_run_copy("roundtrip");
    let adapter = run_copy.display().to_string();
    let (ok, stdout, stderr) = run_python(PROOF_DRIVER, &["--adapter", &adapter]);
    let _ = std::fs::remove_file(&run_copy);
    assert!(
        ok,
        "Python interop proof failed\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    // Non-vacuity of the assertion itself: the driver must have emitted the
    // observed round-trip facts, not just exited 0.
    for marker in [
        "ROUNDTRIP_OK",
        "SQLITE3_REOPEN_OK",
        "SQLITE_SCHEMA_VERSION=31",
        "ERROR_STRUCTURED",
        "LIFECYCLE_CLEAN",
        // DF-HRS-112: the batch + model-usage wire legs now run as part of
        // the default proof; their markers are asserted like the rest so a
        // dispatch-arm regression in src/bin/session_adapter.rs fails the
        // battery instead of only the standalone run.
        "BATCH_WIRE_OK",
        "MODEL_USAGE_WIRE_OK",
        "HRS112_WIRE_CLEAN",
        // DF-HRS-119: the projection leg is driven through the PUBLIC
        // client method and pinned to the message list it counts.
        "GET_SESSION_WIRE_OK",
        "GET_SESSION_BATCH_WIRE_OK",
    ] {
        assert!(
            stdout.contains(marker),
            "proof driver did not report {marker}; stdout was:\n{stdout}"
        );
    }
}

// ────────────────────────────────────────────────────────────────────────
// Protocol-level checks driven directly by Rust (no Python): the boundary
// contract the Python client depends on.
// ────────────────────────────────────────────────────────────────────────

fn adapter_command() -> Command {
    let mut cmd = Command::new(adapter_path());
    cmd.current_dir(repo_root());
    cmd
}

/// Write frames, close stdin, collect all responses (one JSON per line).
fn speak(cmd: &mut Command, frames: &[serde_json::Value]) -> Vec<serde_json::Value> {
    use std::io::Write;
    use std::process::Stdio;

    cmd.stdin(Stdio::piped())
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

#[test]
fn hrs003_protocol_hello_reports_boundary_and_schema() {
    let frames = vec![serde_json::json!({"id": 1, "method": "hello"})];
    let responses = speak(&mut adapter_command(), &frames);
    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    assert_eq!(
        result["protocol"].as_str(),
        Some("hermes-rs/session-adapter/1")
    );
    assert_eq!(result["language"].as_str(), Some("rust"));
    assert_eq!(result["knows_schema_version"].as_i64(), Some(31));
    assert!(
        result["boundary"]
            .as_str()
            .unwrap_or("")
            .contains("process"),
        "boundary statement must name the process boundary"
    );
}

#[test]
fn hrs003_protocol_unknown_method_fails_closed_nonzero() {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = adapter_command();
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn adapter binary");
    {
        let stdin = child.stdin.as_mut().expect("stdin piped");
        stdin
            .write_all(b"{\"id\": 1, \"method\": \"make_me_a_sandwich\"}\n")
            .expect("write frame");
    }
    let out = child.wait_with_output().expect("collect adapter output");
    assert!(
        !out.status.success(),
        "unknown method must fail closed (non-zero exit), got {}",
        out.status
    );
    assert_eq!(out.status.code(), Some(6));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("fail-closed exit 6 at method \"make_me_a_sandwich\""),
        "stderr must name the rejected method: {stderr}"
    );
}

#[test]
fn hrs003_protocol_malformed_json_fails_closed_nonzero() {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = adapter_command();
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn adapter binary");
    {
        let stdin = child.stdin.as_mut().expect("stdin piped");
        stdin.write_all(b"{not json at all\n").expect("write frame");
    }
    let out = child.wait_with_output().expect("collect adapter output");
    assert!(
        !out.status.success(),
        "malformed frame must fail closed, got {}",
        out.status
    );
    assert_eq!(out.status.code(), Some(4));
}

// ────────────────────────────────────────────────────────────────────────
// Non-vacuity: the deliberately broken case must FAIL with a structured
// error (never a crash, never a false success), and the failing output is
// captured into docs/evidence/hrs003/ (same pattern as HRS-008/010).
// ────────────────────────────────────────────────────────────────────────

#[test]
fn hrs003_broken_case_fails_and_evidence_is_written() {
    let run_copy = adapter_run_copy("broken");
    let adapter = run_copy.display().to_string();
    // Write to a temp path first, then compare against the tracked file.
    // Only overwrite if content actually changed — this stops every test
    // run from dirtying the working tree with a regenerated timestamp.
    let temp_transcript = std::env::temp_dir().join(format!(
        "hrs003-broken-case-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let (ok, stdout, stderr) = run_python(
        PROOF_DRIVER,
        &[
            "--mode",
            "broken",
            "--out",
            temp_transcript.to_str().expect("temp path is valid UTF-8"),
            "--adapter",
            &adapter,
        ],
    );
    let _ = std::fs::remove_file(&run_copy);
    assert!(
        ok,
        "the broken-case driver itself must complete its negative proof\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    let evidence = std::fs::read_to_string(&temp_transcript)
        .expect("broken-case evidence transcript exists in temp path");
    assert!(
        evidence.contains("BROKEN_CASE_VERIFIED"),
        "transcript must record the verified negative outcome:\n{evidence}"
    );
    assert!(
        evidence.contains("SessionRowMissing"),
        "transcript must name the structured error class:\n{evidence}"
    );
    // Only update the tracked file if substantive content actually changed.
    // The transcript contains volatile lines (Date, Adapter binary path) that
    // change every run — strip them before comparing so we don't dirty the
    // working tree with regenerated timestamps.
    let stable_evidence = evidence
        .lines()
        .filter(|line| !line.starts_with("Date:") && !line.starts_with("Adapter binary:"))
        .collect::<Vec<_>>()
        .join("\n");
    let tracked_path = repo_root().join(BROKEN_TRANSCRIPT);
    let should_update = match std::fs::read_to_string(&tracked_path) {
        Ok(existing) => {
            let stable_existing = existing
                .lines()
                .filter(|line| !line.starts_with("Date:") && !line.starts_with("Adapter binary:"))
                .collect::<Vec<_>>()
                .join("\n");
            stable_existing != stable_evidence
        }
        Err(_) => true, // file doesn't exist yet
    };
    if should_update {
        std::fs::copy(&temp_transcript, &tracked_path)
            .expect("copy temp transcript to tracked path");
    }
    let _ = std::fs::remove_file(&temp_transcript);
}

// ────────────────────────────────────────────────────────────────────────
// Documented-surface guard (DF-HRS-119): specs/python-interop.md §4 is the
// contract a Python consumer codes against, so every DATA method it lists
// must exist as a callable public method of the reference client — and only
// the explicitly protocol-only frames may be absent. Both spec tables are
// PARSED here (no hand-copied method list) so a new §4 method cannot ship
// unclassified, and a documented method cannot go missing from the client
// again (the defect this row fixes: `get_session` on the wire, absent from
// the client, `AttributeError` for anyone following the docs).
// ────────────────────────────────────────────────────────────────────────

fn spec_text() -> String {
    std::fs::read_to_string(repo_root().join(SPEC)).expect("read specs/python-interop.md")
}

/// The slice of `text` that starts at `heading` and stops at the next
/// occurrence of `stop` (a literal that begins a following heading).
fn heading_region<'a>(text: &'a str, heading: &str, stop: &str) -> &'a str {
    let start = text
        .find(heading)
        .unwrap_or_else(|| panic!("{SPEC}: missing heading {heading:?}"));
    let end = text[start..]
        .find(stop)
        .unwrap_or_else(|| panic!("{SPEC}: heading {heading:?} is not followed by {stop:?}"));
    &text[start..start + end]
}

/// Trimmed cell vectors for every markdown table row in `region`. Outer pipes
/// are stripped first; inner pipes (a `|` inside a result cell, e.g. the
/// `end_session` status alternation) stay inside their cell and therefore
/// cannot shift the first column.
fn table_rows(region: &str) -> Vec<Vec<String>> {
    region
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().trim_matches('`').trim().to_string())
                .collect()
        })
        .collect()
}

fn is_identifier(cell: &str) -> bool {
    !cell.is_empty() && cell.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// Methods documented in the §4 wire-contract table (the table that precedes
/// §4.1). Rows whose first cell is not a bare identifier — the header, the
/// separator, and the trailing "anything else" fail-closed row — are skipped.
fn documented_wire_methods() -> Vec<String> {
    let text = spec_text();
    let region = heading_region(&text, "## 4. Wire contract", "\n### ");
    let mut methods = Vec::new();
    for row in table_rows(region) {
        let first = row.first().map(String::as_str).unwrap_or("");
        if is_identifier(first) {
            methods.push(first.to_string());
        }
    }
    methods
}

/// The §4.2 client-surface manifest as (wire method, client surface) pairs.
/// A client surface of `protocol-only` marks a frame deliberately not exposed
/// as a client method.
fn documented_client_surface() -> Vec<(String, String)> {
    let text = spec_text();
    let region = heading_region(&text, "### 4.2 Reference client surface", "\n## ");
    let mut pairs = Vec::new();
    for row in table_rows(region) {
        if row.len() < 2 {
            continue;
        }
        let (wire, client) = (row[0].as_str(), row[1].as_str());
        if is_identifier(wire) && !client.is_empty() {
            pairs.push((wire.to_string(), client.to_string()));
        }
    }
    pairs
}

/// Run a Python snippet from the repo root; returns (ok, stdout, stderr).
fn run_python_snippet(code: &str) -> (bool, String, String) {
    let out = Command::new(python_bin())
        .arg("-c")
        .arg(code)
        .current_dir(repo_root())
        .output()
        .expect("spawn python3 for the client surface check");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn hrs003_documented_surface_matches_client() {
    // ── the spec's two tables must describe the same wire methods ───────
    let wire: BTreeSet<String> = documented_wire_methods().into_iter().collect();
    let manifest = documented_client_surface();
    let manifest_wire: BTreeSet<String> = manifest.iter().map(|(m, _)| m.clone()).collect();
    assert!(
        wire.len() >= 15,
        "parsed only {} method(s) from the §4 wire table — the parser or the table shape \
         changed: {wire:?}",
        wire.len()
    );
    assert!(
        wire.contains("get_session"),
        "§4 must document the get_session wire method: {wire:?}"
    );
    assert_eq!(
        wire, manifest_wire,
        "specs/python-interop.md §4 and its §4.2 client-surface manifest must list the same \
         methods — a method missing from §4.2 is unclassified for the client"
    );

    // ── only handshake/liveness frames may be protocol-only ────────────
    let protocol_only: BTreeSet<String> = manifest
        .iter()
        .filter(|(_, client)| client == "protocol-only")
        .map(|(m, _)| m.clone())
        .collect();
    let expected_protocol_only: BTreeSet<String> = ["hello".to_string(), "ping".to_string()]
        .into_iter()
        .collect();
    assert_eq!(
        protocol_only, expected_protocol_only,
        "§4.2 may classify ONLY the handshake/liveness frames (hello, ping) as protocol-only; \
         a data method waved through as protocol-only is exactly the drift this guard exists \
         to stop"
    );

    // ── every data method must be a public client method, by name ───────
    let client_pairs: Vec<&(String, String)> = manifest
        .iter()
        .filter(|(_, client)| client != "protocol-only")
        .collect();
    assert!(
        client_pairs
            .iter()
            .any(|(wire, client)| wire == "get_session" && client == "get_session"),
        "§4.2 must map the get_session wire method onto SessionAdapter.get_session"
    );
    let client_src = std::fs::read_to_string(repo_root().join(CLIENT))
        .expect("read the reference client source");
    for (wire_name, client_name) in &client_pairs {
        let needle = format!("    def {client_name}(");
        assert!(
            client_src.contains(&needle),
            "specs/python-interop.md §4 lists `{wire_name}` on the wire and §4.2 maps it to \
             `SessionAdapter.{client_name}`, but {CLIENT} has no {:?} — the documented client \
             surface drifted",
            needle.trim()
        );
    }

    // ── and callable on the real class (not just a def in the text) ─────
    let names = client_pairs
        .iter()
        .map(|(_, client)| format!("{client:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let code = format!(
        "import sys\n\
sys.path.insert(0, 'scripts')\n\
from hrs003_client import SessionAdapter\n\
names = [{names}]\n\
missing = [n for n in names if not callable(getattr(SessionAdapter, n, None))]\n\
if missing:\n    print('SURFACE_MISSING', missing)\n    sys.exit(1)\n\
print('SURFACE_OK', len(names))\n"
    );
    let (ok, stdout, stderr) = run_python_snippet(&code);
    assert!(
        ok,
        "the documented client surface is not callable on SessionAdapter\n--- stdout ---\n\
         {stdout}\n--- stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains(&format!("SURFACE_OK {}", client_pairs.len())),
        "surface probe did not confirm {} callable methods; stdout was:\n{stdout}",
        client_pairs.len()
    );
}
