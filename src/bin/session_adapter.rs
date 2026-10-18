//! HRS-003 Python interoperability adapter: process-boundary server over
//! newline-delimited JSON-RPC on stdio.
//!
//! The Python client (scripts/hrs003_client.py, stdlib-only) launches the
//! `hermes-rs-session-adapter` binary as a subprocess and drives the
//! HRS-010 session-store operations (specs/persistence-shim.md §6.1)
//! through it. Everything crossing the boundary is a JSON value: Python
//! code needs no Rust types, no build toolchain, and no CPython extension
//! module — and the boundary is impossible to mistake for an ABI embed.
//!
//! One frame per request, one per response:
//!   {"id": 1, "method": "hello"}                                → request
//!   {"id": 1, "result": {...}} | {"id": 1, "error": {...}}      → response
//!
//! The server is single-session (one open store per process), fail-closed
//! on protocol errors, and never crashes on a bad call: store errors map
//! to structured `error` responses (error/cause/detail), not process exit.
//! Lifecycle: stdout flushes per frame; `shutdown` exits 0; unknown method
//! / malformed JSON exit non-zero (fail-closed, TEST_ADAPTER_CONTRACT.md §5).

use std::io::{BufRead, Write};

use hermes_rs::persistence::stock_sqlite::{StockSqliteBackend, StockSqliteStore};
use hermes_rs::persistence::{
    AppendMessage, EndSessionOutcome, OpenOptions, Patience, PersistenceError, SessionStore,
    StoreError,
};

const PROTOCOL: &str = "hermes-rs/session-adapter/1";
const KNOWS_SCHEMA: i64 = hermes_rs::persistence::SCHEMA_VERSION_KNOWN;

fn main() {
    // The process is the unit of teardown (§3.5): a clean exit releases
    // every SQLite POSIX lock; any fail-closed path exits non-zero.
    std::process::exit(run());
}

fn run() -> i32 {
    // Argument scan: at most the first argument matters (--protocol); any
    // other argument is a usage error. (Clippy: a `for` loop over `.take(1)`
    // expresses this without a never-repeating `while let`.)
    if let Some(arg) = std::env::args().nth(1) {
        if arg == "--protocol" {
            println!("{PROTOCOL}");
            return 0;
        }
        eprintln!("unknown argument: {arg}");
        return 2;
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut store: Option<StockSqliteStore> = None;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => return 3, // stdin closed uncleanly
        };
        if line.trim().is_empty() {
            continue;
        }
        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                // Fail closed on malformed input (no id to reply to).
                eprintln!("malformed request frame: {e}");
                return 4;
            }
        };
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let params = req
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        match dispatch(&mut store, &method, params) {
            Dispatch::Reply(result) => {
                let frame = serde_json::json!({ "id": id, "result": result });
                if emit(&mut stdout, &frame).is_err() {
                    return 5; // stdout closed: the client is gone
                }
            }
            Dispatch::ErrReply(err) => {
                let frame = serde_json::json!({ "id": id, "error": err });
                if emit(&mut stdout, &frame).is_err() {
                    return 5;
                }
            }
            Dispatch::Exit(code) => {
                if let Some(s) = store.as_mut() {
                    let _ = s.close();
                }
                if code != 0 {
                    eprintln!("fail-closed exit {code} at method {method:?}");
                }
                return code;
            }
        }
    }
    // EOF on stdin without a shutdown frame: still clean up the store.
    if let Some(s) = store.as_mut() {
        let _ = s.close();
    }
    0
}

fn emit(stdout: &mut std::io::Stdout, frame: &serde_json::Value) -> std::io::Result<()> {
    let mut line = serde_json::to_string(frame)
        .map_err(|_| std::io::Error::other("frame serialization failed"))?;
    line.push('\n');
    stdout.write_all(line.as_bytes())?;
    stdout.flush()
}

enum Dispatch {
    Reply(serde_json::Value),
    ErrReply(serde_json::Value),
    Exit(i32),
}

fn dispatch(
    store: &mut Option<StockSqliteStore>,
    method: &str,
    params: serde_json::Value,
) -> Dispatch {
    match method {
        "hello" => Dispatch::Reply(serde_json::json!({
            "protocol": PROTOCOL,
            "knows_schema_version": KNOWS_SCHEMA,
            "language": "rust",
            "boundary": "process (subprocess, newline-delimited JSON-RPC over stdio)"
        })),
        "ping" => Dispatch::Reply(serde_json::json!({ "pong": true })),
        "open" => match open_store(store, &params) {
            Ok(reply) => Dispatch::Reply(reply),
            Err(err) => Dispatch::ErrReply(err),
        },
        "ensure_session" => {
            let Some(sid) = str_param(&params, "session_id") else {
                return missing_param("session_id");
            };
            let source = params
                .get("source")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();
            let model = params
                .get("model")
                .and_then(|m| m.as_str())
                .map(String::from);
            call(store, |s| {
                s.write(Patience::WRITE, &mut |txn| {
                    txn.ensure_session(&sid, &source, model.as_deref())
                })
                .map(|()| serde_json::json!({ "ensured": sid }))
            })
        }
        "append_message" => {
            let Some(sid) = str_param(&params, "session_id") else {
                return missing_param("session_id");
            };
            let msg = AppendMessage {
                role: params
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("user")
                    .to_string(),
                content: params
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(String::from),
                tool_calls: params
                    .get("tool_calls")
                    .and_then(|c| c.as_str())
                    .map(String::from),
                tool_call_id: params
                    .get("tool_call_id")
                    .and_then(|c| c.as_str())
                    .map(String::from),
                tool_name: params
                    .get("tool_name")
                    .and_then(|c| c.as_str())
                    .map(String::from),
                ..AppendMessage::default()
            };
            call(store, |s| {
                s.write(Patience::TRANSCRIPT, &mut |txn| {
                    txn.append_message(&sid, &msg)
                })
                .map(|row_id| serde_json::json!({ "message_id": row_id }))
            })
        }
        "get_messages" => {
            let Some(sid) = str_param(&params, "session_id") else {
                return missing_param("session_id");
            };
            let limit = params
                .get("limit")
                .and_then(|l| l.as_u64())
                .map(|l| l as usize);
            call(store, |s| {
                s.read(&mut |txn| {
                    let rows = txn.get_messages(&sid, false, false, limit, 0)?;
                    let messages: Vec<serde_json::Value> = rows
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "id": m.id,
                                "session_id": m.session_id,
                                "role": m.role,
                                "content": m.content,
                                "timestamp": m.timestamp,
                                "token_count": m.token_count,
                                "message_uid": m.message_uid,
                                "active": m.active,
                                "compacted": m.compacted,
                            })
                        })
                        .collect();
                    Ok(serde_json::json!({ "messages": messages }))
                })
            })
        }
        "get_session" => {
            let Some(sid) = str_param(&params, "session_id") else {
                return missing_param("session_id");
            };
            call(store, |s| {
                s.read(&mut |txn| txn.get_session(&sid)).map(|row| {
                    serde_json::to_value(
                        row.map(|r| {
                            serde_json::json!({
                                "id": r.id,
                                "source": r.source,
                                "model": r.model,
                                "message_count": r.message_count,
                                "tool_call_count": r.tool_call_count,
                                "input_tokens": r.input_tokens,
                                "output_tokens": r.output_tokens,
                                "api_call_count": r.api_call_count,
                                "estimated_cost_usd": r.estimated_cost_usd,
                                "started_at": r.started_at,
                                "ended_at": r.ended_at,
                                "end_reason": r.end_reason,
                                "parent_session_id": r.parent_session_id,
                                "session_key": r.session_key,
                            })
                        })
                        .unwrap_or(serde_json::Value::Null),
                    )
                    .unwrap_or(serde_json::Value::Null)
                })
            })
        }
        "end_session" => {
            let Some(sid) = str_param(&params, "session_id") else {
                return missing_param("session_id");
            };
            let reason = params
                .get("end_reason")
                .and_then(|r| r.as_str())
                .unwrap_or("session_reset")
                .to_string();
            call(store, |s| {
                s.write(Patience::WRITE, &mut |txn| txn.end_session(&sid, &reason))
                    .map(|outcome| match outcome {
                        EndSessionOutcome::Ended { generation_bumped } => serde_json::json!({
                            "status": "ended",
                            "generation_bumped": generation_bumped
                        }),
                        EndSessionOutcome::AlreadyEnded => {
                            serde_json::json!({ "status": "already_ended" })
                        }
                    })
            })
        }
        "search" => {
            let Some(query) = str_param(&params, "query") else {
                return missing_param("query");
            };
            let limit = params
                .get("limit")
                .and_then(|l| l.as_u64())
                .map(|l| l as usize)
                .unwrap_or(10);
            call(store, |s| {
                s.read(&mut |txn| {
                    let hits = txn.search_messages(&query, limit, 0)?;
                    let hits_json: Vec<serde_json::Value> = hits
                        .iter()
                        .map(|h| {
                            serde_json::json!({
                                "message_id": h.message_id,
                                "session_id": h.session_id,
                                "role": h.role,
                                "snippet": h.snippet,
                                "rank": h.rank,
                            })
                        })
                        .collect();
                    Ok(serde_json::json!({ "hits": hits_json }))
                })
            })
        }
        "append_messages_batch" => {
            let Some(sid) = str_param(&params, "session_id") else {
                return missing_param("session_id");
            };
            let Some(messages) = params.get("messages").and_then(|m| m.as_array()) else {
                return missing_param("messages");
            };
            let msgs: Vec<AppendMessage> = messages
                .iter()
                .map(|m| AppendMessage {
                    role: m.get("role").and_then(|r| r.as_str()).unwrap_or("user").to_string(),
                    content: m.get("content").and_then(|c| c.as_str()).map(String::from),
                    tool_calls: m.get("tool_calls").and_then(|c| c.as_str()).map(String::from),
                    tool_call_id: m.get("tool_call_id").and_then(|c| c.as_str()).map(String::from),
                    tool_name: m.get("tool_name").and_then(|c| c.as_str()).map(String::from),
                    timestamp: m.get("timestamp").and_then(|t| t.as_f64()),
                    token_count: m.get("token_count").and_then(|t| t.as_i64()),
                    message_uid: m.get("message_uid").and_then(|u| u.as_str()).map(String::from),
                    ..AppendMessage::default()
                })
                .collect();
            call(store, |s| {
                s.write(Patience::TRANSCRIPT, &mut |txn| {
                    txn.append_messages_batch(&sid, &msgs)
                })
                .map(|report| serde_json::json!({ "first_id": report.first_id, "appended": report.appended }))
            })
        }
        "record_model_usage" => {
            let Some(sid) = str_param(&params, "session_id") else {
                return missing_param("session_id");
            };
            let delta = hermes_rs::persistence::ModelUsageDelta {
                model: params.get("model").and_then(|m| m.as_str()).unwrap_or("").to_string(),
                billing_provider: params.get("billing_provider").and_then(|p| p.as_str()).unwrap_or("").to_string(),
                billing_base_url: params.get("billing_base_url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                billing_mode: params.get("billing_mode").and_then(|m| m.as_str()).unwrap_or("").to_string(),
                task: params.get("task").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                api_call_count: params.get("api_call_count").and_then(|c| c.as_i64()).unwrap_or(0),
                input_tokens: params.get("input_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                output_tokens: params.get("output_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                cache_read_tokens: params.get("cache_read_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                cache_write_tokens: params.get("cache_write_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                reasoning_tokens: params.get("reasoning_tokens").and_then(|t| t.as_i64()).unwrap_or(0),
                estimated_cost_usd: params.get("estimated_cost_usd").and_then(|c| c.as_f64()).unwrap_or(0.0),
                cost_status: params.get("cost_status").and_then(|s| s.as_str()).map(String::from),
                cost_source: params.get("cost_source").and_then(|s| s.as_str()).map(String::from),
            };
            call(store, |s| {
                s.write(Patience::WRITE, &mut |txn| {
                    txn.record_model_usage(&sid, &delta)
                })
                .map(|()| serde_json::json!({ "ok": true }))
            })
        }
        "checkpoint" => call(store, |s| {
            s.checkpoint_passive()
                .map(|_| serde_json::json!({ "ok": true }))
        }),
        "backup" => {
            let Some(dest) = str_param(&params, "dest") else {
                return missing_param("dest");
            };
            call(store, |s| {
                s.backup(std::path::Path::new(&dest))
                    .map(|_| serde_json::json!({ "ok": true }))
            })
        }
        "close" => {
            let reply = call(store, |s| {
                s.close().map(|_| serde_json::json!({ "closed": true }))
            });
            // The store handle is gone only after a successful close reply;
            // a failed close leaves it in place for retry/diagnosis.
            if matches!(reply, Dispatch::Reply(_)) {
                *store = None;
            }
            reply
        }
        "shutdown" => {
            if let Some(s) = store.as_mut() {
                let _ = s.close();
            }
            *store = None;
            Dispatch::Exit(0)
        }
        other => {
            // Fail closed: an unknown protocol verb is a hard non-zero exit,
            // never a guessed answer (TEST_ADAPTER_CONTRACT.md §5).
            let _ = other;
            Dispatch::Exit(6)
        }
    }
}

fn str_param(params: &serde_json::Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn missing_param(key: &str) -> Dispatch {
    Dispatch::ErrReply(serde_json::json!({
        "error": "InvalidInput",
        "cause": "unknown",
        "detail": format!("params.{key} (string) is required")
    }))
}

fn open_store(
    store: &mut Option<StockSqliteStore>,
    params: &serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    if store.is_some() {
        return Err(serde_json::json!({
            "error": "AlreadyOpen",
            "cause": "unknown",
            "detail": "this adapter serves one store per process; use close/shutdown first"
        }));
    }
    let Some(path) = params
        .get("path")
        .and_then(|p| p.as_str())
        .map(String::from)
    else {
        return Err(serde_json::json!({
            "error": "InvalidInput",
            "cause": "unknown",
            "detail": "params.path (string) is required"
        }));
    };
    let read_only = params
        .get("read_only")
        .and_then(|c| c.as_bool())
        .unwrap_or(false);
    let opts = if read_only {
        OpenOptions::read_only(&path)
    } else {
        let mut o = OpenOptions::read_write(&path);
        o.create_if_missing = params
            .get("create")
            .and_then(|c| c.as_bool())
            .unwrap_or(false);
        o
    };
    match StockSqliteBackend::open(opts) {
        Ok((backend_store, open)) => {
            let capabilities = serde_json::json!({
                "journal": format!("{:?}", open.capabilities.journal),
                "fts_base": open.capabilities.fts.base.is_ok(),
                "fts_trigram": open.capabilities.fts.trigram.is_ok(),
                "fts_cjk": open.capabilities.fts.cjk.is_ok(),
                "checkpoint_passive_only": open.capabilities.checkpoint_passive_only,
                "file_format_standard_sqlite": open.capabilities.file_format_standard_sqlite,
                "bulk_parameter_limit": open.capabilities.bulk_parameter_limit,
            });
            let report = serde_json::json!({
                "schema_version_before": open.report.schema_version_before,
                "schema_version_after": open.report.schema_version_after,
                "columns_reconciled": open.report.columns_reconciled,
                "degraded": open.report.degraded,
                "created_new_store": open.report.created_new_store,
            });
            *store = Some(backend_store);
            Ok(serde_json::json!({
                "capabilities": capabilities,
                "report": report
            }))
        }
        Err(e) => Err(persistence_error_json(&e)),
    }
}

/// Run `f` against the open store, mapping the result or the store error
/// onto a reply. A missing store is a structured NotOpen error, not a crash.
fn call<F>(store: &mut Option<StockSqliteStore>, f: F) -> Dispatch
where
    F: FnOnce(&mut StockSqliteStore) -> Result<serde_json::Value, PersistenceError>,
{
    match store.as_mut() {
        None => Dispatch::ErrReply(serde_json::json!({
            "error": "NotOpen",
            "cause": "unknown",
            "detail": "no store is open; call open first"
        })),
        Some(s) => match f(s) {
            Ok(v) => Dispatch::Reply(v),
            Err(e) => Dispatch::ErrReply(persistence_error_json(&e)),
        },
    }
}

/// Map the shim's error taxonomy (persistence-shim.md §3.4) to structured
/// JSON across the boundary. `error` is a stable machine code; `cause` is
/// the upstream cause-tuple member; `detail` is human-readable.
fn persistence_error_json(e: &PersistenceError) -> serde_json::Value {
    use hermes_rs::persistence::PersistenceError as PE;
    let (error, cause, detail): (&str, &str, String) = match e {
        PE::Busy { kind } => (
            "Busy",
            "locked",
            format!("store busy ({kind:?}); retryable"),
        ),
        PE::LeaseLost { lease } => (
            "LeaseLost",
            "compression",
            format!("lease lost ({lease:?})"),
        ),
        PE::Corrupt { scope } => (
            "Corrupt",
            "corrupt",
            format!("corrupt store ({scope:?}); sticky quarantine"),
        ),
        PE::DiskFull => ("DiskFull", "disk", "disk full".into()),
        PE::Replaced => (
            "Replaced",
            "replaced",
            "database file replaced under the handle".into(),
        ),
        PE::WalGenerationLost => (
            "WalGenerationLost",
            "deleted_wal",
            "WAL generation lost (stranded sidecars)".into(),
        ),
        PE::SessionRowMissing => (
            "SessionRowMissing",
            "session_row_missing",
            "no such session row".into(),
        ),
        PE::FtsUnavailable { reason } => ("FtsUnavailable", "fts_index", reason.clone()),
        PE::Schema { need } => (
            "Schema",
            "unknown",
            format!("schema admission failed ({need:?})"),
        ),
        PE::Unsupported { capability } => (
            "Unsupported",
            "unknown",
            format!("capability ruled out ({capability:?})"),
        ),
        PE::UnknownOutcome => (
            "UnknownOutcome",
            "unknown",
            "settlement unknown; re-check state before retry".into(),
        ),
        PE::Io(io) => ("Io", "unknown", format!("io error: {io}")),
        PE::InvalidInput(m) => ("InvalidInput", "unknown", m.clone()),
        PE::CompressionClosed => (
            "CompressionClosed",
            "compression_closed",
            "session is compression-closed; transcript writes refused".into(),
        ),
    };
    serde_json::json!({ "error": error, "cause": cause, "detail": detail })
}

// The §6.1 operation set this adapter exposes; a compile-time witness that
// the wire surface is a subset of the trait's contract (method names mirror
// these exactly). StoreError itself never crosses the boundary: the store
// already lifts it into PersistenceError inside write()/read().
type WriteCallback<T> =
    dyn FnMut(&mut dyn hermes_rs::persistence::WriteTxn) -> Result<T, StoreError>;

const _: () = {
    fn _witness<T>(_f: &dyn Fn(&mut WriteCallback<T>)) {}
    let _ = _witness::<i64>;
};
