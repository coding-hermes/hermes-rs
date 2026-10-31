---
name: hermes-rs-usage
description: Use when working in or with hermes-rs (the Rust foundation crate) — how to build it, how to drive the SQLite session store from Rust or Python, what is proven vs deferred, and the traps found by real use (search sanitizer, fresh-store report, backup precondition, install smoke on a clean box).
---

# Using hermes-rs

hermes-rs is an **independent Rust foundation** for Hermes-shaped session
persistence. It is not Hermes Agent and does not implement agent behaviour. What
exists and is proven: an engine-neutral persistence contract over stock SQLite,
and one tested process boundary that lets stdlib Python drive it.

Read `README.md` (scope/status), `specs/persistence-shim.md` (§3 boundary, §6
slice, §6.4 explicit non-claims), `specs/python-interop.md` (the wire contract
and its supported/unsupported table) before claiming anything about parity.

## Build and run

```sh
cargo build            # library + hermes-rs + hermes-rs-session-adapter
cargo test --workspace # the battery (needs a `sqlite3` CLI on PATH — see traps)
cargo run --bin hermes-rs                       # CLI scaffold: prints the stage line, rc=0
cargo run --bin hermes-rs -- --help             # show CLI usage and options
cargo run --bin hermes-rs -- --version          # print the crate version
cargo run --bin hermes-rs-session-adapter        # explicitly run the session adapter
```

The target dir may be redirected: this fleet sets `CARGO_TARGET_DIR`
(`/var/tmp/hermes-cargo-target`), so binaries land in
`$CARGO_TARGET_DIR/{debug,release}/…`, **not** `./target/`. A build that exits 0
and leaves `./target` empty is not a failed build — resolve the path from the
variable and check the binary's mtime against `git log -1` before trusting a
probe.

## Driving the session store from Rust

```rust
use hermes_rs::persistence::{OpenOptions, Patience, SessionStore, AppendMessage};
use hermes_rs::persistence::stock_sqlite::StockSqliteBackend;

let opts = OpenOptions::read_write(path);      // .create_if_missing = true
let (store, open) = StockSqliteBackend::open(opts)?;   // OpenReport + Capabilities
store.write(Patience::TRANSCRIPT, &mut |txn| {
    txn.ensure_session("sess-1", "source", Some("model"))?;
    txn.append_message("sess-1", &AppendMessage { content: Some("hi".into()), role: Some("user".into()), ..Default::default() })
})?;
store.read(&mut |txn| txn.search_messages("hi", 10, 0))?;
```

One write = one durable transaction. `append_message` returns the durable rowid
and moves the session counters in the same transaction.

## Driving it from Python (the HRS-003 boundary)

```sh
cargo build --release --bins
python3 <checkout>/scripts/hrs003_e2e_proof.py --adapter $CARGO_TARGET_DIR/release/hermes-rs-session-adapter
```

Odd lines in, odd lines out, one JSON object per line; `scripts/hrs003_client.py`
is the reference stdlib client (60 lines of real protocol, no Rust toolchain, no
pip install). `hello` → `protocol`, `knows_schema_version`, `boundary` (a
process boundary, deliberately not an embed).

## Traps

Re-verified by real use at `f7fba49` (run 4, 2026-10-23 — a 60,000-message
archive through this Python boundary: `docs/dogfood/2026-10-23-run4-integration.md`,
evidence in `docs/evidence/dogfood-20261023/`). The eight defects this section used
to list as OPEN (`DF-HRS-104..111`) are closed and their fixes re-proved live — do
NOT repeat the old workarounds. What is still true:

- **Wrong-typed `open` flags are REFUSED, not ignored** (re-proved run 4 at HEAD; the fix
  is `04822cc`, which landed *after* run 3 wrote this section, `DF-HRS-114`). `create:
  "yes"` / `create: 1` answer `InvalidInput: params.create must be a JSON boolean; got
  string|number`, likewise `read_only: "true"` for `params.read_only`; no store is opened
  and no file is created. Absent or `null` still defaults to `false`, and a real
  `read_only: true` handle refuses writes with `InvalidInput: read-only store: attempt to
  write a readonly database`. Pass JSON booleans with their own type — a caller who
  follows that rule is not silently upgraded to a write-capable handle any more.

- **A fresh store now reports honestly**: `created_new_store: true` on creation and
  `degraded: []` (the census is clean). You may branch on both — the old "always false /
  always degraded" coaching is gone (`DF-HRS-106`, `DF-HRS-107`).
- **`backup` overwrites an existing destination** — a nightly loop to the same path is fine
  now, and the snapshot is page-consistent (`DF-HRS-108`).
- **A session's start time cannot be set.** `ensure_session(session_id, source, model)`
  takes no timestamp on either surface, so every imported session is stamped with the
  import wall-clock and `started_at` can postdate its own messages' timestamps. Archived
  sessions therefore cannot be time-ordered or date-filtered faithfully (`DF-HRS-115`).
- **`session_model_usage` is write-only through the documented API.** `record_model_usage`
  works (and folds into the session projection), but no `ReadTxn` method and no adapter
  method returns the rollup — read it with raw SQL against `session_model_usage`; its
  columns, in the DDL's order, are `session_id`, `model`, `billing_provider`,
  `billing_base_url`, `billing_mode`, `task`, `api_call_count`, `input_tokens`,
  `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens`,
  `estimated_cost_usd`, `actual_cost_usd`, `cost_status`, `cost_source`, `first_seen`,
  `last_seen` (as `src/persistence/upstream_ddl.rs` declares them; `DF-HRS-115`).
- **`search` is token-based, never substring.** Word-class queries are safe now (`notes`,
  `brand`, `android` all match exactly), paths and punctuation survive (`src/main.rs`,
  `state.db`, `C++`, `O'Brien`), CJK is served through the trigram index even though
  `fts_cjk=false`, and a trailing `*` is passed to FTS as a **prefix** query (so `state*`
  also returns `states`, `statement`…). A zero-result search for a token that is present is
  no longer expected — verify against a LIKE query before blaming the sanitizer.
- **Multiple words mean AND; `AND` / `OR` work; quotes do NOT mean phrase.** Measured on
  a control store with known contents (run 4): `connection pool` returns rows holding
  *both* tokens, `connection OR pool` returns either, word order does not matter, and
  `"connection pool"` behaves exactly like the unquoted pair — a message with other words
  *between* the two matched. `state*` (trailing `*`) is the only proximity-free trick
  documented here.
- **Hits are display-ready**: each is `{message_id, rank, role, session_id, snippet}` where
  `snippet` wraps the matched term in `>>>…<<<`. There is no `content` field — a client that
  reads `hit["content"]` renders every hit blank (run 4's first probe bug). `rank` is BM25
  (lower/more negative = better match) and results arrive in rank order.
- **`get_session` is public on the reference client.** `session.get_session(sid)` answers the
  projection `specs/python-interop.md` §4 documents (counters, timestamps, `end_reason`), and
  `None` when the id has no row — a null projection is an answer, not an `AdapterError`.
  `DF-HRS-119` closed the earlier gap where the method existed on the wire only; §4.2 now maps
  every data method onto a client method and `tests/hrs003_python_interop.rs` machine-checks
  that mapping (wire table ↔ §4.2 manifest ↔ callable `SessionAdapter` attribute). `hello` /
  `ping` stay protocol-only by design: speak the frame if you need the protocol banner or a
  liveness probe. Never reach for `_call(...)` for a documented method (§4.2 is exhaustive).
- **Unknown method / malformed frame kill the process** (exit 6 / exit 4, by design). A client
  must `poll()` before writing or it dies with a broken pipe; §4 lists `ProcessDead` as a
  client-side code for exactly this.
- **`shutdown` replies nothing** — it closes silently with exit 0 (`specs/python-interop.md`
  §4's table says "process exits 0"). Do not block waiting for a reply frame there.
- **`cargo test` without a `sqlite3` CLI**: the suite stays green and 5 tests SKIP with a
  recorded reason in `$CARGO_TARGET_DIR/hrs010-skipped-tests.log` (`DF-HRS-110`). That log
  can glue two records onto one line when test threads write concurrently (`DF-HRS-116`).
- **CARGO_TARGET_DIR**: unchanged — this fleet sets it
  (`/var/tmp/hermes-cargo-target`), so binaries land there, not in `./target/`.

## Don't claim these

No agent functionality; no generic CPython/ABI/embedding support (PyO3 and C-ABI
are rejected by design); no Python 3.14 claim (3.11 proven, 3.12 incidentally
green on a fresh box); no multi-process *concurrent* write sharing across the
boundary (`advisory_generation_guard: false`); not the full upstream `SessionDB`
API; no engine/percentage/parity claim — Turso was evaluated and **not adopted**,
PostgreSQL is out of scope. Any performance statement lives in
`benchmarks/results/` with its methodology, never in prose.

## Where the records live

`specs/` (contracts) · `docs/dogfood/` (real-use reports + diagnostics) ·
`docs/evidence/` (captured transcripts, install logs) ·
`.coding-hermes/board/tasks.jsonl` (open findings) ·
`docs/upstream-releases/PARITY.md` (upstream tracking, no support implied).
