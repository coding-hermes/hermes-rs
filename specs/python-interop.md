# Python interoperability boundary (HRS-003)

| Field | Value |
|---|---|
| Status | COMPLETE (one proven boundary + evidence; explicitly not a universal-ABI claim) |
| Date | 2026-10-15 |
| Task | HRS-003 (Roadmap Gate 6: "Prove one Python integration") |
| Upstream pin | **v0.22.0** @ commit `e06df9c2eae6901f4c88f464e0ae6e753c0428b8` (tag object `84094bd903791c35867605e28e25075baa293adc`, commit dated 2026-10-13) — same pin as every HRS-001/002 row; provenance in `UPSTREAM_BASELINE.md` §1 |
| Rust surface proven against | The HRS-010 session-store slice (`src/persistence/` — `StockSqliteBackend`/`StockSqliteStore`, §6.1 operations, 22 cargo tests at HRS-010 close), NOT a hypothetical API |
| Companion docs | `specs/persistence-shim.md` (HRS-002 boundary), `specs/COMPATIBILITY_MATRIX.md`, `specs/HERMES_RS_COMPATIBILITY.md` (AC-3), `specs/ROADMAP.md` (Gate 6), `specs/TEST_ADAPTER_CONTRACT.md` (fail-closed discipline) |
| Proof code | `src/bin/session_adapter.rs` (adapter server), `scripts/hrs003_client.py` (stdlib-only Python client), `scripts/hrs003_e2e_proof.py` (proof driver), `tests/hrs003_python_interop.rs` (cargo-wired tests) |
| Evidence | `docs/evidence/hrs003/broken-case-transcript.txt` (deliberately broken case FAILING), plus the green run recorded in §5 |

Normative language: MUST / MUST NOT / SHOULD / MAY as in RFC 2119. "Upstream" means
the pinned Hermes Agent v0.22.0 tree. The project directive governs: **"Existing
Python libraries must remain usable through tested interoperability adapters."**
Nothing here claims generic CPython ABI compatibility, embedding support, or any
performance characteristic.

## 1. Purpose and scope

1. Inventory the pinned upstream package/extension seams relevant to
   persistence (§2) — what an existing Python program actually imports,
   on which runtime, with which requirements.
2. Choose the adapter boundary against the REAL Rust surface from HRS-010,
   comparing process/protocol options with reasons (§3), and record
   rejected alternatives (§4).
3. Prove ONE end-to-end integration through the chosen boundary: a real
   Python process (CPython 3.11, stdlib only) driving the Rust session
   store — structured data round-trip, structured error propagation,
   lifecycle, and the independent file-level read-back (§5).
4. State supported and unsupported Python behavior explicitly (§6).

Out of scope: any second subsystem, any embedding story, any claim about
arbitrary Python libraries beyond the proven surface, any backend-engine
question (HRS-009), any performance statement.

## 2. Inventory: the upstream Python seams relevant to persistence (v0.22.0 @ `e06df9c2`)

Sources: `specs/UPSTREAM_BASELINE.md` (pin provenance + harness inventory),
`docs/upstream-releases/v0.22.2.json` + `docs/upstream-releases/PARITY.md`
(newer-release tracking), and direct reads of the pinned checkout at
`/tmp/hermes-upstream-pin-hrs001` (read-only reference, same checkout
`specs/TEST_ADAPTER_CONTRACT.md` §1 cites). Every symbol below was read at
the pin; path:symbol/line references are stable citations.

### 2.1 Package identity and distribution

| Field | Value at the pin | Source |
|---|---|---|
| Project name | `hermes-agent` | `pyproject.toml:6-7` (`name = "hermes-agent"`, `version = "0.0.0"` — source-tree layout, release versioning lives in release channels/tags, not the sdist version) |
| Repository | `NousResearch/hermes-agent` | `UPSTREAM_BASELINE.md` §1 |
| Python requirement | `>=3.11,<3.15` | `pyproject.toml:15` (`requires-python`) |
| Managed runtime (v0.22.0 release) | Python 3.14 | `UPSTREAM_BASELINE.md` §1 release-notes summary |
| Dev/test runtime at the pin | CPython 3.14.5 + pytest 9.1.1 + pytest-asyncio 1.3.0 | `TEST_ADAPTER_CONTRACT.md` §2 (collection environment) |
| Entry points | `hermes = hermes_cli.main:main`, `hermes-agent = agent.legacy_cli:main`, `hermes-acp = acp_adapter.entry:main` | `pyproject.toml:556-559` (`[project.scripts]`) |
| Dependency policy | every direct dep exact-pinned `==X.Y.Z` (no ranges) | `pyproject.toml:19-33` (policy comment) |
| Optional native-ish deps | `psutil` imported with graceful `None` fallback in the state store; `asyncpg==0.31.0` (linux) used only by the PostgreSQL portability tool | `hermes_state.py:83-85`; `pyproject.toml:284` |
| The persistence seam's own requirement | **stdlib only**: `sqlite3` (C extension shipped with CPython), no third-party import on the store's hot path | `hermes_state.py:8-27` import block |

Key structural fact: the state store is **not a separately distributed PyPI
package**. It is the in-tree `hermes_state.py` + `hermes_state_*.py` module
family (33 modules, ~20.6K lines; `hermes_state_common.py` etc.) consumed as
library code by upstream components (gateway, CLI, desktop backend). An
"existing Python library integration" for persistence therefore means: a
Python program that today imports `SessionDB` and talks to `state.db`. That
seam is what the adapter must keep usable.

### 2.2 Persistence API surface (the seam the adapter mirrors)

| Upstream entry point | Role | Reference at the pin |
|---|---|---|
| `SessionDB` | the store handle; one writer connection, pooled read-only connections, WAL + explicit `BEGIN IMMEDIATE` write transactions | `hermes_state.py:460` (class), `:790-792` (connect kwargs), `:994-1075` (`_execute_write`) |
| `ensure_session(session_id, source, model, ...)` | upsert; returns the id | `hermes_state_sessions.py:464` |
| `append_message(session_id, role, content, ...)` | transcript append inside the write transaction; returns the durable rowid | `hermes_state_messages.py:389` |
| `get_messages(session_id, ...)` | ascending-rowid read with active/compacted inclusion flags | `hermes_state_messages.py:1392` |
| `end_session(session_id, end_reason)` | first end wins (`ended_at IS NULL` predicate) | `hermes_state_sessions.py:520` |
| `search_messages(query, ...)` | FTS5 MATCH (BM25 default) with LIKE fallbacks | `hermes_state_search.py:1063` |
| `close()` | drain token deltas, PASSIVE checkpoint, registry refcount release | `hermes_state.py:1507` |
| Migrations | `schema_version` single-row table (`SCHEMA_VERSION = 31`), additive `_reconcile_columns` on every open, deferred indexes/triggers | `hermes_state_schema.py:756` (`_reconcile_columns`), `:988-990` (version read/insert), `hermes_state_common.py:283` |
| File contract | `state.db` (+`-wal`/`-shm` sidecars) at `get_hermes_home()`; REAL epoch timestamps, TEXT ids, JSON-as-TEXT, AUTOINCREMENT rowids | `hermes_state.py:177,199-200`; `specs/persistence-shim.md` §2.1/§2.2 |

### 2.3 Runtime requirements relevant to the boundary choice

- **No native extension of upstream's own** is required for the store:
  `sqlite3` ships with CPython; `psutil` is optional with a coded fallback.
  Consequence: a Python client that only needs persistence can stay
  stdlib-only — the adapter proof exploits exactly this (§5).
- **Upstream creates sidecars and locks around the DB file** (WAL, OFD-lock
  guard, quarantine/repair locks). Two live writers on one file from two
  languages is a supported co-existence shape ONLY through SQLite's own
  locking (proven for the file level by HRS-010's fixture round-trip);
  the adapter additionally funnels WRITES through one implementation at a
  time to stay clear of the WAL-generation discipline upstream enforces
  (`persistence-shim.md` §2.7/§2.11).
- **Homes and guards**: upstream refuses a pytest-context process touching
  a production `state.db` (`hermes_state_guard.py`); any Python-side tooling
  the Rust runtime ships must respect the same isolation conventions
  (isolated `HERMES_HOME` in tests — `TEST_ADAPTER_CONTRACT.md` §4.2).
- **This repo's tested interpreter**: CPython 3.11.15, `sqlite3` module 2.6.0
  over SQLite lib 3.53.1 (recorded in the evidence transcript, §5). Upstream's
  floor (`>=3.11`) is respected; upstream's managed 3.14 runtime is NOT
  exercised by this proof and is listed as untested (§6).

## 3. Adapter boundary decision

### 3.1 The real Rust surface being exposed

HRS-010 landed the session store behind the HRS-002 engine-neutral shim:
`StockSqliteBackend::open(OpenOptions) -> (StockSqliteStore, OpenStore)`,
with `SessionStore::write/read` transactions exposing exactly the §6.1
operation set (`ensure_session`, `append_message`, `get_messages`,
`search_messages`, `end_session`, `record_model_usage`,
`append_messages_batch`) plus
`checkpoint_passive` / `backup` / `close` / `capabilities` /
`open_report` (`src/persistence/api.rs`, `src/persistence/stock_sqlite.rs`).
The store is a Rust struct with `&mut self` semantics on close and a
`Mutex<Connection>` inside — process-internal state, no network surface.

### 3.2 Requirements the boundary must satisfy

| # | Requirement | Source |
|---|---|---|
| R1 | Existing Python code reaches the store without a Rust toolchain or Rust types | contract directive + AC-3 |
| R2 | Structured data crosses the boundary (create/append/read/reopen) | Gate 6 wording |
| R3 | Errors are structured and typed across the boundary (upstream cause taxonomy), never a crash | `persistence-shim.md` §3.4; Gate 6 "errors" |
| R4 | Clean lifecycle: start/stop, no orphaned process or lock | Gate 6 "lifecycle" |
| R5 | Fail-closed testability: a broken target FAILS, absence/crash is never a pass | `TEST_ADAPTER_CONTRACT.md` §5 |
| R6 | No generic-ABI claim is implied; support is explicit and tested | contract §3 edge cases; task contract |
| R7 | Engine-neutrality survives: no engine type crosses the Python-facing boundary | `persistence-shim.md` §3.1 engine-leak rule |
| R8 | No build-time coupling that would force every Python user to compile Rust (or vice versa) | Gate 6 "installation" |

### 3.3 Options compared

| Option | R1 | R2 | R3 | R4 | R5 | R6 | R7 | R8 | Notes |
|---|---|---|---|---|---|---|---|---|---|
| **A. Subprocess + newline-delimited JSON-RPC over stdio** (chosen) | Yes — plain `subprocess` + `json` | Yes — JSON values | Yes — JSON error objects mapped 1:1 from the §3.4 taxonomy | Yes — process exit IS the teardown; SQLite locks release on exit | Yes — crash/exit ≠ pass; stderr + exit codes observable | Yes — the boundary is visibly a protocol, never mistakable for an embed | Yes — adapter methods mirror the shim's plain-data API | Yes — client is stdlib-only; server is a normal cargo binary | Parent-child stdio needs no ports, no daemon, no discovery |
| B. In-process FFI via PyO3 (`import hermes_rs`) | Only with a compiled wheel per platform | Yes | Partial — Rust panics/unwinds need manual bridging to Python exceptions | Weaker — store lifetime is a Python object's lifetime; a leaked handle holds SQLite locks | Hard — a crashed embed takes the host process down; fail-closed semantics blur | **No** — an importable Rust module IS an ABI-era claim; extension-module support varies sharply across CPython versions (upstream spans 3.11–3.14) | Yes | No — every Python user needs a built extension for their exact interpreter | Also couples our release cadence to CPython's C-API stability |
| C. C-ABI shared library (`ctypes`/`cffi`) | Only with `ctypes` glue | Yes (POD types) | Weak — errno-style codes, no rich error objects without more glue | Weaker — manual `init/close` discipline; leaks hold locks | Medium | Risky — `ctypes` glue invites "it's just C, works everywhere" claims; struct/pointer layout is its own ABI surface | Partial | No — same built-artifact distribution problem as B | Strictly worse than B for our error-taxonomy needs |
| D. Local socket (TCP/Unix) + JSON-RPC | Yes | Yes | Yes | Weaker — port/sock file lifecycle, stale-socket cleanup, daemonization questions | Yes | Yes | Yes | Yes | All of A's costs plus socket lifecycle; buys nothing for a parent-child relationship where the client spawns the server |
| E. File-level co-access only (Python `sqlite3` reads/writes `state.db` directly) | Yes | Yes | Partial — SQLite error strings, not the upstream cause taxonomy | N/A (SQLite's problem; WAL generation discipline applies) | Medium | No overclaim but ALSO no adapter: the Rust logic (guards, taxonomy, capabilities) is bypassed | n/a | Yes | Already PROVEN at the file level by HRS-010 (Python-produced fixture reopened by Rust, Rust-written file reopened by `sqlite3`) — it stays supported as the file-format baseline (`persistence-shim.md` §4) but it is not a programmable interop boundary |

### 3.4 Decision

**Option A — subprocess + newline-delimited JSON-RPC over stdio** — is the
proven boundary. Concretely:

- Server: `hermes-rs-session-adapter` (`src/bin/session_adapter.rs`), a
  cargo binary that speaks the protocol on stdin/stdout, one open store per
  process, fail-closed exits on protocol violations (exit 4 malformed frame,
  6 unknown method; clean `shutdown` exit 0).
- Client: `scripts/hrs003_client.py` — CPython stdlib only (`subprocess`,
  `json`); raises `AdapterError(error, cause, detail)` from structured
  server replies.
- Error mapping: every `PersistenceError` variant (the §3.4 taxonomy) maps
  to `{error, cause, detail}` where `cause` is the upstream cause-tuple
  member (`locked`, `compression`, `compression_closed`, `corrupt`, `disk`,
  `replaced`, `deleted_wal`, `session_row_missing`, `fts_index`, `unknown`).
- Protocol version: `hermes-rs/session-adapter/1`, reported by `hello`
  together with `knows_schema_version` and an explicit
  `boundary: "process (subprocess, newline-delimited JSON-RPC over stdio)"`
  statement, so no consumer can mistake the seam for an embed.

### 3.5 Rejected alternatives — recorded reasons

- **PyO3 / in-process embedding (B)**: rejected for THIS gate, not
  necessarily forever. Reasons: (1) it would make the proof an implicit
  statement about CPython extension-module behavior across the upstream
  support range (3.11–3.14) — precisely the generic-ABI claim the directive
  forbids; (2) it couples every Python consumer to a compiled wheel for
  their exact interpreter (R8), while upstream's own store needs zero
  compiled deps beyond CPython's `sqlite3` (§2.1); (3) fail-closed semantics
  (R5) blur inside one process — a store panic kills the caller; the process
  boundary makes every failure mode observable and classifiable. Revisit
  only with a named, tested interpreter matrix and its own gate.
- **C-ABI library (C)**: rejected — worst error-story (R3) and the same
  distribution cost as B, with hand-managed memory and layout drift risks.
- **Local socket (D)**: rejected — strictly more lifecycle surface (ports,
  socket files, stale-socket cleanup, daemonization) for zero benefit over
  stdio in the parent-child shape the proof uses.
- **File-level co-access only (E)**: not a rejection — it is the ALREADY
  PROVEN file-format baseline (HRS-010 fixtures; `persistence-shim.md` §4.1
  "file-compatible": Python reference and Rust alternate on one file without
  conversion). It is recorded here because Gate 6 asks what the BOUNDARY is:
  file co-existence is the data-plane guarantee; option A is the
  programmable control-plane. The proof (§5) exercises BOTH and keeps the
  writer roles disjoint per phase.

## 4. Wire contract (`hermes-rs/session-adapter/1`)

Request: one JSON object per line, `{"id": <n>, "method": <name>,
"params": {...}}`. Response: `{"id": <n>, "result": {...}}` or
`{"id": <n>, "error": {"error": <code>, "cause": <upstream-cause>,
"detail": <text>}}`.

| Method | Params | Result / error | Mirrors |
|---|---|---|---|
| `hello` | — | protocol, `knows_schema_version`, `language`, `boundary` | — |
| `ping` | — | `{"pong": true}` | — |
| `open` | `path`, `create`, `read_only` | capabilities + open report (schema versions, degraded list) | `StockSqliteBackend::open` |
| `ensure_session` | `session_id`, `source?`, `model?` | `{"ensured": id}` | §6.1 `ensure_session` |
| `append_message` | `session_id`, `role?`, `content?`, `tool_calls?`, `tool_call_id?`, `tool_name?` | `{"message_id": rowid}` | §6.1 `append_message` |
| `get_messages` | `session_id`, `limit?` | `{"messages": [...]}` ascending rowid | §6.1 `get_messages` |
| `get_session` | `session_id` | session projection or `null` | §6.1 (read) |
| `end_session` | `session_id`, `end_reason?` | `{"status": "ended"|"already_ended", "generation_bumped": bool}` | §6.1 `end_session` (first end wins) |
| `search` | `query`, `limit?` | `{"hits": [...]}` BM25-rank order | §6.1 `search` |
| `checkpoint` | — | `{"ok": true}` (PASSIVE-only) | §2.7 policy |
| `backup` | `dest` | `{"ok": true}` (backup-API snapshot) | §2.10 |
| `close` | — | `{"closed": true}`; store handle dropped | §6.1 `close` |
| `shutdown` | — | process exits 0 | lifecycle |
| anything else | — | **process exits 6** (fail closed) | `TEST_ADAPTER_CONTRACT.md` §5 |

Error codes crossing the boundary: `Busy`, `LeaseLost`, `Corrupt`,
`DiskFull`, `Replaced`, `WalGenerationLost`, `SessionRowMissing`,
`FtsUnavailable`, `Schema`, `Unsupported`, `UnknownOutcome`, `Io`,
`InvalidInput`, `CompressionClosed`, plus adapter-level `NotOpen`,
`AlreadyOpen`, `ProcessDead`, `ProtocolError` (client-side). Mapping table
lives in `persistence_error_json` (`src/bin/session_adapter.rs`); `cause`
values follow the upstream cause tuple (`persistence-shim.md` §3.4,
`hermes_state_errors.py:137-140`).

## 5. The end-to-end proof

### 5.1 What runs

`cargo test --test hrs003_python_interop` (wired into the repo battery —
no separate runner needed) executes, in one cargo invocation:

1. **`hrs003_python_roundtrip_and_sqlite_readback`** — spawns REAL CPython
   (`python3`, override with `HERMES_RS_PYTHON`) running
   `scripts/hrs003_e2e_proof.py` against a task-unique copy of the adapter
   binary (shared-`CARGO_TARGET_DIR` clobber protection). The Python driver:
   - **create/append/read**: opens a fresh store (`create=True`), verifies
     schema v31 admission, `ensure_session`, two appends (one carrying
     `tool_calls`/`tool_call_id`/`tool_name` JSON — the structured-data
     conversion case), reads them back in rowid order;
   - **reopen**: closes the first adapter process (clean `shutdown`), opens
     the SAME file through a SECOND adapter process, re-reads both messages,
     verifies the session row (`message_count == 2`, `end_reason`), verifies
     the backup snapshot contains both rows;
   - **independent read-back**: the stdlib `sqlite3` module — a DIFFERENT
     SQLite stack than the one the Rust build links (3.53.1 vs the bundled
     3.53.2 amalgamation) — opens the same file, checks
     `PRAGMA integrity_check = ok`, reads both acknowledged rows, and reads
     `schema_version = 31`;
   - **structured error**: appends into a nonexistent session →
     `AdapterError(SessionRowMissing, cause=session_row_missing)` — a
     structured error, not a crash; the store answers a follow-up write
     normally;
   - **lifecycle**: verifies zero adapter processes remain (exact-argv0
     `/proc` scan, self-match-safe) and zero `-wal`/`-shm` sidecars after
     the final clean close;
   - the driver prints machine-checkable markers (`ROUNDTRIP_OK`,
     `SQLITE3_REOPEN_OK`, `SQLITE_SCHEMA_VERSION=31`, `ERROR_STRUCTURED`,
     `LIFECYCLE_CLEAN`) and the Rust test asserts every marker — exit 0
     alone would not be evidence.
2. **`hrs003_protocol_hello_reports_boundary_and_schema`** — Rust-side
   protocol check: `hello` reports the process boundary and schema 31.
3. **`hrs003_protocol_unknown_method_fails_closed_nonzero`** — an unknown
   method exits 6 and names the method on stderr.
4. **`hrs003_protocol_malformed_json_fails_closed_nonzero`** — a malformed
   frame exits 4.
5. **`hrs003_broken_case_fails_and_evidence_is_written`** — the non-vacuity
   leg: runs the driver's `--mode broken`, which performs the deliberately
   wrong call and captures the failing output to
   `docs/evidence/hrs003/broken-case-transcript.txt` (HRS-008/010 evidence
   pattern). The test then requires that transcript to contain
   `BROKEN_CASE_VERIFIED` and the structured error class — i.e. the broken
   case demonstrably FAILED with the right error, and the failure is
   checked into the repo.

### 5.2 Environment of the recorded run

| Field | Value |
|---|---|
| Date | 2026-10-15 |
| Python | CPython 3.11.15 (`sqlite3` module 2.6.0, SQLite lib 3.53.1) |
| Rust | stable 1.98.0, `rusqlite` 0.40 with `bundled` SQLite 3.53.2 |
| Upstream pin cited | v0.22.0 @ `e06df9c2` (no upstream code executed in the proof; upstream informs the inventory and the file contract) |
| Installation | nothing installed for the Python side (stdlib only — the client imports `subprocess`, `json`, `dataclasses`); the adapter binary is built by the normal cargo build; this is the Gate 6 "installation" proof: zero pip installs, zero C extensions compiled for Python |

### 5.3 Non-vacuity evidence

`docs/evidence/hrs003/broken-case-transcript.txt` records the broken run:
the exact adapter binary, interpreter versions, the mutation (append into a
never-ensured session id), the observed structured error, and the follow-up
`ping` proving the server stayed alive. If the store ever stopped rejecting
that call, the driver writes `BROKEN_CASE_FAILED_TO_FAIL`, exits non-zero,
and the cargo test fails — the proof cannot rot silently into a vacuous pass.

## 6. Supported and unsupported Python behavior (explicit)

### 6.1 Tested and supported (by THIS proof, at THIS pin)

- **CPython 3.11.15** (host interpreter; upstream floor `>=3.11`), the
  CPython standard library only: `subprocess`, `json`, `sqlite3`,
  `dataclasses`, `argparse`, `tempfile`, `os`.
- The §6.1 session-store operation set over the process boundary (§4),
  including structured `tool_calls` JSON payloads and the documented error
  taxonomy (§3.4).
- File-level co-existence: a file written by the Rust store is opened and
  read by a DIFFERENT standard SQLite stack (CPython `sqlite3` 3.53.1) with
  `integrity_check = ok` — the same file-format guarantee HRS-010 proved in
  the reverse direction (Python-produced fixture read by Rust).
- Lifecycle: spawn, drive, clean `shutdown` (exit 0), no orphaned process,
  no residual WAL sidecars or lock artifacts.

### 6.2 Explicitly NOT covered (never claim these)

| Not covered | Why it is not claimed |
|---|---|
| Generic CPython ABI compatibility / in-process embedding (`import hermes_rs`) | Not implemented, not tested; PyO3/C-ABI are REJECTED for this gate (§3.5). One protocol proof does not establish embedding support on any interpreter, let alone the 3.11–3.14 range. |
| Upstream's managed Python 3.14 runtime | Untested here; the proof ran on 3.11.15. Nothing in the boundary is 3.11-specific (pure stdlib, stable APIs), but support is claimed only where tested. |
| Native CPython extensions in the client (numpy-style C modules), CFFI/ctypes consumers, Cython | Out of proof scope; no statement about them is made. |
| asyncio-based clients / event-loop embedding | The client is synchronous; async usage is untested (upstream's own store is sync-first with dedicated token-writer threads, `persistence-shim.md` §2.5). |
| Multi-process write sharing ACROSS the boundary (Python writer + Rust writer concurrently on one file) | Deliberately out of proof: the proof keeps writer roles disjoint per phase. Upstream's cross-process discipline (WAL generation guard, lock holder liveness, `persistence-shim.md` §2.11) is only partially ported (`advisory_generation_guard: false` in the slice's capabilities — recorded, not silent). Concurrent-writer pairing is future work gated on the lockguard port. |
| The full upstream `SessionDB` API | Only the §6.1 surface exists in the slice; upstream's compression chains, leases, outbox, prune/VACUUM, portability tools are NOT exposed and NOT claimed (`persistence-shim.md` §6.4/§7). |
| Any Python LIBRARY beyond the stdlib client | The proof names ONE real package (CPython's stdlib `sqlite3`) and ONE client. Per Gate 6: one example does not establish universal compatibility; each future library integration gets its own tested adapter + matrix row. |
| Performance characteristics of any kind | No benchmark ran; no claim is made (HRS-009 owns measured comparisons). |

## 7. Verification

```
# The interop proof inside the repo battery (needs a Python 3 interpreter on PATH
# or $HERMES_RS_PYTHON):
cargo test --workspace --all-features          # includes tests/hrs003_python_interop.rs
cargo test --test hrs003_python_interop        # just this boundary

# Standalone (manual) driver runs:
python3 scripts/hrs003_e2e_proof.py --adapter <path-to-hermes-rs-session-adapter>
python3 scripts/hrs003_e2e_proof.py --mode broken --out docs/evidence/hrs003/broken-case-transcript.txt \
    --adapter <path-to-hermes-rs-session-adapter>
```

Expected: all five interop tests pass; the driver prints the five markers of
§5.1; the broken-case transcript ends `BROKEN_CASE_VERIFIED`.

## References

- `specs/HERMES_RS_COMPATIBILITY.md` — directive ("Existing Python libraries
  must remain usable through tested interoperability adapters"), AC-3, §3
  edge cases (explicit adapter support, never presumed).
- `specs/ROADMAP.md` — Gate 6 (this task's gate).
- `specs/persistence-shim.md` — HRS-002 boundary (§3), stock-SQLite file
  contract (§4), session-store slice (§6), error taxonomy (§3.4).
- `specs/UPSTREAM_BASELINE.md` — pin provenance; §2 harness inventory.
- `specs/TEST_ADAPTER_CONTRACT.md` — fail-closed adapter discipline (§5),
  pinned checkout path (§1).
- `docs/upstream-releases/v0.22.2.json`, `docs/upstream-releases/PARITY.md`
  — newer-release tracking (no interop claim derived from it).
- Upstream (all at v0.22.0 @ `e06df9c2`, read at
  `/tmp/hermes-upstream-pin-hrs001`, read-only): `pyproject.toml`
  (`requires-python`, `[project.scripts]`, dependency policy), `hermes_state.py`
  (imports, `SessionDB`, `close`), `hermes_state_sessions.py`
  (`ensure_session`, `end_session`), `hermes_state_messages.py`
  (`append_message`, `get_messages`), `hermes_state_search.py`
  (`search_messages`), `hermes_state_schema.py` (`_reconcile_columns`,
  `schema_version`), `hermes_state_errors.py` (cause tuple).
