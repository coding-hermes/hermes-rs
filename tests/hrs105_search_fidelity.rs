//! DF-HRS-105 integration tests: search fidelity for FTS5-special characters.
//!
//! Evidence: `docs/evidence/dogfood-20261017/search-fidelity.log` (fidelity
//! matrix + exact runner commands) and
//! `docs/evidence/dogfood-20261017/zero-hit-fallback.log` (criterion 2).
//!
//! The bug: `sanitize_fts5_query` step 2 DELETED each FTS5-special character
//! with no separator, so the words on either side fused into one token
//! (`src/main.rs` → `srcmain.rs`, `cost=5.50` → `cost5.50`). Step 5 then quoted
//! that fused token as a phrase, so a search answered zero for text the user
//! can see in the store — a silent miss, indistinguishable from a genuine
//! absence. The fix substitutes a SPACE, preserving token boundaries.
//!
//! RED evidence (recorded in the log above, not asserted here): the same
//! fidelity matrix driven against the pre-fix revision `1093dfd` answered
//! 21/31 MISS, and the `ok!` case below answered 0 hits. Against that revision
//! every test in this file fails.
//!
//! What is pinned:
//!   1. all 31 verbatim strings are findable (acceptance floor: 25/31),
//!   2. the zero-hit LIKE fallback is a genuine FALLBACK — it runs only when
//!      MATCH returned nothing, so a query FTS5 CAN answer is never narrowed to
//!      the LIKE arm's row set,
//!   3. the fallback serves the same ROW SET as the FTS arm (hidden
//!      `display_kind` and rewound rows stay invisible), and
//!   4. the fallback means LITERAL presence: LIKE wildcards in the user's term
//!      are escaped, so a hit is text the store actually holds.

use std::path::PathBuf;

use hermes_rs::persistence::{
    AppendMessage, OpenOptions, Patience, ReadTxn, SessionStore, StockSqliteBackend, WriteTxn,
};

/// The 31 verbatim strings from the fidelity matrix, in matrix order.
const CASES: [&str; 31] = [
    "src/main.rs",
    "cost=5.50",
    "ratio 50%",
    "#hashtag",
    "note (see appendix)",
    "a,b;c",
    "ok!",
    "$5",
    "path C:\\Users\\kara",
    "quote 'single'",
    "email a.b@example.com",
    "v1.2.3-rc.1",
    "under_score",
    "dash-term",
    "tilde~x",
    "amp&x",
    "pipe|x",
    "star*",
    "caret^x",
    "at@x",
    "bracket[x]",
    "brace{x}",
    "colon:x",
    "slash/x",
    "plus+x",
    "back\\slash",
    "lt<x",
    "gt>x",
    "semi;x",
    "bang!x",
    "q?x",
];

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hrs105-{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir.join("state.db")
}

/// True when a usable `sqlite3` CLI is on PATH (an absent binary, a
/// non-executable stub and a non-zero `--version` all count as "absent").
/// Tests that verify the row-visibility contract must flip columns the public
/// write API does not expose, exactly like `hrs010_session_store.rs`'s
/// stale-marker test, so they need the CLI; on a bare box they skip with a
/// recorded reason instead of panicking on `Os { code: 2, kind: NotFound }`.
fn has_sqlite3_cli() -> bool {
    std::process::Command::new("sqlite3")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// One process-wide lock, so two threads of this test binary can never
/// interleave the bytes of two records. A single `write_all` reaches the
/// kernel as one `write(2)` for a short record, but a short write would append
/// its remainder *after* whatever another thread wrote in between, so the lock
/// is what actually makes "one record = one line" hold.
static SKIP_LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Directory holding the skip log: `$CARGO_TARGET_DIR` when set, else this
/// crate's own `target/`.
fn skip_log_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
}

/// Append ONE skip record to `log` as exactly one physical line.
///
/// DF-HRS-116: this file carried a second, identical copy of the skip helper
/// (a fix in `hrs010_session_store.rs` alone would have left the same defect
/// class here). `writeln!` emits one `write(2)` per format fragment and
/// `O_APPEND` makes each of *those* atomic, not the record, so concurrent
/// skippers can glue two records onto one line. The whole record, trailing
/// newline included, is built into one `String` first and handed to a single
/// `write_all` while `SKIP_LOG_LOCK` is held.
fn append_skip_record(log: &std::path::Path, reason: &str) {
    // The record must be exactly one line: assert the shape BEFORE writing, so
    // a future edit that drops the newline (or folds a newline into the reason)
    // fails loudly here instead of gluing two records onto one line.
    let line = format!("{reason}\n");
    assert!(
        line.ends_with('\n'),
        "skip-log record must carry its own trailing newline, got {line:?}"
    );
    assert_eq!(
        line.len(),
        reason.len() + 1,
        "skip-log record must be the reason plus exactly one newline, got {line:?}"
    );
    assert!(
        !reason.contains('\n'),
        "a skip reason must not itself span lines, got {reason:?}"
    );
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // A poisoned lock must not silently swallow the evidence that a test was
    // skipped: take the guard back out of the poison and still write.
    let _guard = SKIP_LOG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

/// Answer whether the caller must return early because the `sqlite3` CLI is
/// absent, recording the reason on stderr and in
/// `hrs105-skipped-tests.log` under the Cargo target directory. The write goes
/// through `append_skip_record`, so one skipped test is always exactly one
/// line even when several tests skip at once (DF-HRS-116).
fn skip_without_sqlite3_cli(test: &str) -> bool {
    if has_sqlite3_cli() {
        return false;
    }
    let reason = format!("SKIPPED: {test} — sqlite3 CLI not available on this machine");
    eprintln!("{reason}");
    append_skip_record(&skip_log_dir().join("hrs105-skipped-tests.log"), &reason);
    true
}

/// Seed `rows` into a fresh store. Returns the store, the durable row ids in
/// `rows` order, and the store path (the concrete store type is deliberately
/// private to the backend — callers see the `SessionStore` boundary).
fn seed(tag: &str, rows: &[&str]) -> (impl SessionStore, Vec<i64>, String, PathBuf) {
    let path = scratch(tag);
    let (store, _open) = StockSqliteBackend::open(OpenOptions::read_write(&path)).unwrap();
    let sid = format!("20261017_133000_{tag}");
    let ids = store
        .write(Patience::WRITE, &mut |tx: &mut dyn WriteTxn| {
            tx.ensure_session(&sid, "cli", Some("glm-5.3-flash"))?;
            let mut ids = Vec::new();
            for content in rows {
                ids.push(tx.append_message(
                    &sid,
                    &AppendMessage {
                        role: "user".into(),
                        content: Some((*content).into()),
                        ..Default::default()
                    },
                )?);
            }
            Ok(ids)
        })
        .expect("seed rows");
    (store, ids, sid, path)
}

/// Message ids of the hits that belong to `sid`, ascending.
fn hits(store: &impl SessionStore, sid: &str, query: &str) -> Vec<i64> {
    let mut v: Vec<i64> = store
        .read(&mut |tx: &mut dyn ReadTxn| tx.search_messages(query, 50, 0))
        .unwrap_or_else(|e| panic!("search {query:?} errored: {e:?}"))
        .iter()
        .filter(|h| h.session_id == sid)
        .map(|h| h.message_id)
        .collect();
    v.sort_unstable();
    v
}

// ────────────────────────────────────────────────────────────────────────
#[test]
fn df105_fidelity_matrix_all_31_verbatim_strings_are_findable() {
    let rows: Vec<String> = CASES
        .iter()
        .enumerate()
        .map(|(i, text)| format!("row {i:02}: {text} end"))
        .collect();
    let refs: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    let (store, ids, sid, _path) = seed("matrix", &refs);
    assert_eq!(ids.len(), 31, "every row got a durable id");

    let misses: Vec<&str> = CASES
        .iter()
        .enumerate()
        .filter(|(i, text)| !hits(&store, &sid, text).contains(&ids[*i]))
        .map(|(_, text)| *text)
        .collect();

    // Acceptance floor is 25/31 (6 misses allowed); the fix is expected to be
    // complete on the bundled SQLite build. The same probe against the pre-fix
    // revision answered 21/31 MISS.
    assert!(
        misses.is_empty(),
        "DF-HRS-105: {}/31 fidelity strings unfindable (floor: <=6 misses, \
         pre-fix 21) — misses: {misses:?}",
        misses.len()
    );
    drop(store);
}

// ────────────────────────────────────────────────────────────────────────
#[test]
fn df105_zero_hit_like_fallback_is_a_fallback_not_a_replacement() {
    let (store, ids, sid, _path) = seed(
        "fallback",
        &[
            "case A alpha marker row",
            "case A alpha!beta marker row",
            "case B aok!x marker row",
        ],
    );

    // CASE A — FTS5 CAN answer, so the fallback must not narrow the answer.
    // 'alpha!' sanitizes to 'alpha' (step 2 turns '!' into a space) and BOTH
    // rows carry the token 'alpha', while the RAW term 'alpha!' is present in
    // only one. A replacement-shaped implementation (LIKE for every search)
    // answers 1 here; a genuine fallback answers 2.
    assert_eq!(
        hits(&store, &sid, "alpha!"),
        vec![ids[0], ids[1]],
        "the FTS5 arm must answer a query it can answer (2 rows), never the \
         LIKE arm's narrower 1-row answer"
    );

    // CASE B — FTS5 cannot answer but the raw literal is present: the fallback
    // must fire. unicode61 indexes 'aok!x' as the tokens 'aok' and 'x' ('!' is
    // no boundary), so the sanitized query 'ok' matches NEITHER token, while
    // the literal 'ok!' IS in the row. Pre-fix: 0 hits (the silent miss).
    assert_eq!(
        hits(&store, &sid, "ok!"),
        vec![ids[2]],
        "zero-hit fallback must serve the row holding the raw literal 'ok!'"
    );

    // CASE C — honest zeros stay zero: the fallback must not invent hits.
    assert!(hits(&store, &sid, "zzz-no-such-text-zzz").is_empty());
    assert!(hits(&store, &sid, "zzz/no-such/text").is_empty());
    drop(store);
}

// ────────────────────────────────────────────────────────────────────────
#[test]
fn df105_fallback_serves_the_same_rows_as_the_fts_arm() {
    if skip_without_sqlite3_cli("df105_fallback_serves_the_same_rows_as_the_fts_arm") {
        return;
    }
    let (store, ids, sid, path) = seed(
        "visibility",
        &[
            "visible src/main.rs marker",
            "hiddenonly src/main.rs marker",
            "rewoundonly src/main.rs marker",
        ],
    );
    drop(store); // clean close: the CLI then sees a quiescent file

    // hidden scaffolding the person never saw, and a rewound row
    // (active=0, compacted=0) — the FTS arm filters both.
    let sql = format!(
        "UPDATE messages SET display_kind='hidden' WHERE id={}; \
         UPDATE messages SET active=0, compacted=0 WHERE id={};",
        ids[1], ids[2]
    );
    let out = std::process::Command::new("sqlite3")
        .arg(&path)
        .arg(&sql)
        .output()
        .expect("run sqlite3");
    assert!(out.status.success(), "flag flip failed: {out:?}");

    let (store, _open) = StockSqliteBackend::open(OpenOptions::read_write(&path)).unwrap();

    // Positive control: a VISIBLE row holding the term is found.
    assert_eq!(
        hits(&store, &sid, "visible src/main.rs"),
        vec![ids[0]],
        "the visible control row must be found"
    );
    assert_eq!(
        hits(&store, &sid, "src/main.rs"),
        vec![ids[0]],
        "the FTS arm serves only visible rows"
    );

    // Each query below answers zero through the FTS arm *because of* the
    // visibility filter — the term lives only in the hidden / rewound row and
    // the index still holds it — so the zero-hit fallback runs on a term that
    // IS present in those rows. It must not hand them back.
    assert!(
        hits(&store, &sid, "hiddenonly src/main.rs").is_empty(),
        "display_kind='hidden' rows must stay invisible through the fallback"
    );
    assert!(
        hits(&store, &sid, "rewoundonly src/main.rs").is_empty(),
        "rewound (active=0, compacted=0) rows must stay invisible through the fallback"
    );
    drop(store);
}

// ────────────────────────────────────────────────────────────────────────
#[test]
fn df105_fallback_requires_literal_presence_not_a_wildcard_match() {
    // 'aokXx' holds no literal 'ok_x', but an UNESCAPED LIKE '%ok_x%' matches it
    // ('_' is a single-character wildcard) — a hit for text the store does not
    // contain, the same silent wrong answer as the original bug pointing the
    // other way. FTS5 cannot answer 'ok_x' here: the sanitized phrase 'ok x'
    // needs tokens 'ok' then 'x', and unicode61 gives 'aokXx' one token and
    // 'aok_x' the tokens 'aok' + 'x'. So the query reaches the fallback, which
    // must return exactly the row where the literal really appears.
    let (store, ids, sid, _path) = seed("escape", &["aokXx marker row", "aok_x marker row"]);
    assert_eq!(
        hits(&store, &sid, "ok_x"),
        vec![ids[1]],
        "'ok_x' must match the live literal in 'aok_x' and NOT wildcard-match 'aokXx'"
    );
    drop(store);
}

// ────────────────────────────────────────────────────────────────────────
// DF-HRS-116: this file's skip helper carried the same non-atomic
// `writeln!`-on-the-log write as `hrs010_session_store.rs`; the test below
// pins the sibling copy too, so "one record = one line" cannot regress in
// either file unnoticed.
// ────────────────────────────────────────────────────────────────────────

/// Read the log and assert it holds exactly `expected` records in exactly
/// `expected` physical lines, each line being exactly one record.
fn assert_skip_log_lines(log: &std::path::Path, expected: usize) {
    let raw = std::fs::read_to_string(log).expect("read skip log");
    assert!(raw.ends_with('\n'), "log must end with a newline: {raw:?}");
    let lines: Vec<&str> = raw.lines().collect();
    assert_eq!(
        lines.len(),
        expected,
        "log holds {} lines: {lines:?}",
        lines.len()
    );
    assert_eq!(
        raw.matches("SKIPPED:").count(),
        expected,
        "records were glued onto one line: {raw:?}"
    );
    let mut names = std::collections::BTreeSet::new();
    for line in &lines {
        let body = line
            .strip_prefix("SKIPPED: ")
            .unwrap_or_else(|| panic!("line is not a skip record: {line:?}"));
        let (name, reason) = body
            .split_once(" — ")
            .unwrap_or_else(|| panic!("record has no ' — ' separator: {line:?}"));
        assert!(
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "first field must be exactly one test name, got {name:?} in {line:?}"
        );
        assert!(!reason.is_empty(), "record carries no reason: {line:?}");
        assert!(
            !reason.contains("SKIPPED:"),
            "two records were glued onto one line: {line:?}"
        );
        assert!(
            names.insert(name.to_string()),
            "duplicate record for {name}"
        );
    }
}

#[test]
fn df116_concurrent_skips_write_one_line_per_record() {
    const THREADS: usize = 32;
    const ROUNDS: usize = 4;
    let dir = std::env::temp_dir().join(format!(
        "hrs105-skiplog-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create skip-log scratch dir");
    let log = dir.join("hrs105-skipped-tests.log");
    for round in 0..ROUNDS {
        std::thread::scope(|scope| {
            for t in 0..THREADS {
                let log = log.clone();
                scope.spawn(move || {
                    let reason = format!(
                        "SKIPPED: t{round}_{t} — sqlite3 CLI not available on this machine"
                    );
                    append_skip_record(&log, &reason);
                });
            }
        });
        assert_skip_log_lines(&log, (round + 1) * THREADS);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
