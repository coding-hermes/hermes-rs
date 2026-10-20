# Dogfood run 3 — a real Rust consumer, and a re-verification of the recorded traps

Date: 2026-10-20 · Target HEAD: `7331204` (main, pushed) · Lane: `hermes-rs-dogfood`
Run 1 (2026-10-14) covered the two spec-stage scaffold scripts; run 2 (2026-10-17)
drove the HRS-003 boundary from a **stdlib-Python consumer written outside the repo**.
This run takes the surface neither touched: **the Rust library used as a library, by a
real consumer crate written outside the repo**, plus the archive-import job that no
previous run attempted — and, because every defect the usage skill tells users to work
around is now closed, an independent re-verification of that coaching.

Evidence: `docs/evidence/dogfood-20261020/` (reproducible with `run_evidence.sh.txt`).
Board rows filed this run: `DF-HRS-113` … `DF-HRS-117`.

## 1. Promise (the null hypothesis)

`specs/persistence-shim.md` §3/§6 + `skills/hermes-rs-usage/SKILL.md`: *a Rust program can
open a state.db-compatible store through the engine-neutral `SessionStore` API, write
sessions and messages into it, search them back, record per-model usage, snapshot the
store — and a second surface (the stdio adapter) can read the same file.*

## 2. What I actually built (outside the repo)

`/home/kara/hrs-dogfood-20261020/` — a real consumer crate, `hrspack`, that does a job I
have: **pack Hermes-shaped session transcripts (JSONL, one session per line) into a
state.db-compatible archive, then search it, record model usage and snapshot it.** It uses
only the documented caller-facing API (`OpenOptions::read_write`, `StockSqliteBackend::open`,
`SessionStore::{write,read,backup,close}`, `WriteTxn::{ensure_session, append_message,
append_messages_batch, record_model_usage, end_session}`, `ReadTxn::{get_session,
get_messages, search_messages}`). No repo module is imported and no internal helper is used.

Corpus: 200 sessions × 20 messages = **4,000 messages**, self-generated
(`gen_corpus.py.txt`) with realistic agent prose, Rust/JSON code blocks, paths
(`src/main.rs`, `state.db`), FTS5-special punctuation, Spanish accents, CJK (数据库), emoji
(🔒) and two needle strings — plus a per-query **oracle** computed from the corpus text with
token-boundary semantics, so search is judged against ground truth rather than against itself.

## 3. Results

### 3.1 The headline job works, and the file is faithful

| leg | command | observed |
|---|---|---|
| import (batch) | `hrspack import corpus.jsonl out/batch.db batch` | 200 sessions / 4,000 messages, `opened … schema_after=31 created_new_store=true degraded=[] quarantined=None` |
| import (per-message, same one-txn-per-session shape) | `… single` | same counts, same open report |
| independent read-back | `verify_store.py` (system sqlite3 3.53.4, a different SQLite build) | `integrity_check=ok`, `journal_mode=wal`, `schema_version=31`, sessions=200, messages=4000, usage_rows=200, message_uid all 32-hex, timestamps REAL, session counters consistent, **4,000/4,000 messages byte-identical to the corpus (0 mismatches)** — CJK, emoji, accents and specials survive exactly |
| search fidelity | 20 probes vs the corpus oracle, limit 5000 | **19/20 exact** (`missing=0 extra=0`): `notes` 610/610, `brand` 658/658, `android` 419/419, `notebook` 455/455, `src/main.rs` 854/854, `state.db` 979/979, `数据库` 113/113, `contraseña` 109/109, `🔒` 199/199, `C++` 109/109, `O'Brien` 112/112, needles 111/111 |
| cross-surface read | adapter (stdlib Python, `probe_adapter_read.py`) on the Rust-written store | `created_new_store=false degraded=[]`, session projection identical, `search` 5 hits with a correct snippet |
| backup | `hrspack backup out/batch.db out/snap.db` twice to the **same** destination | both `ok`, snapshot holds 4,000 messages |
| usage rollup | `record_model_usage` then read the session back | session projection moved exactly by the delta: (7526,512,10,0.0001) → (7637,734,12,0.0124); the rollup row itself is only readable with raw SQL (see DF-HRS-115) |

The one probe that differs from my oracle — `state*` (expected 130 substring hits, got
1,109) — is **inherited FTS5 prefix-operator semantics**, not a defect: the sanitizer lets a
user's trailing `*` through as a prefix query, which is the useful behaviour. Recorded here
so nobody files it as a bug.

### 3.2 The recorded trap list is stale — every workaround the usage skill teaches is unnecessary at HEAD

`skills/hermes-rs-usage/SKILL.md` still tells a user/agent that eight defects are open and
to work around them. All eight are `complete` on the board and I re-verified the fixes
independently from the consumer side at `7331204`:

| row | skill's coaching at run 2 | observed at HEAD (this run) |
|---|---|---|
| DF-HRS-104 | `notes`→`es`: word-class queries silently miss | `notes` 610/610, `brand` 658/658 — no truncation |
| DF-HRS-105 | verbatim strings with FTS5 specials unfindable | `src/main.rs`, `state.db`, `C++`, `O'Brien` — all exact |
| DF-HRS-106 | `created_new_store` always false | `created_new_store=true` on a fresh store, `false` on reopen |
| DF-HRS-107 | every fresh store opens `degraded` | `degraded=[]` on a fresh store (census clean) |
| DF-HRS-108 | `backup` refuses an existing destination | second backup to the same dest: **ok** |
| DF-HRS-109 | `append_messages_batch` unimplemented | implemented; 4,000 messages imported through it |
| DF-HRS-110 | the README smoke verb fails on a box without `sqlite3` | with a `sqlite3` shim that fails `--version`: 23 passed / 0 failed, 5 SKIPPED with reasons in `hrs010-skipped-tests.log` |
| DF-HRS-111 | the tree is dirty after every `cargo test` | `git status --porcelain` empty after two full suite runs |

Suite at HEAD with a real `sqlite3`: **43 passed / 0 failed / 1 ignored, 49 s**. So the repo
is healthier than its own coaching. The coaching is now corrected in the skill (this run),
and the staleness itself is filed as `DF-HRS-113` because it will recur if nothing keeps
the skill in step with the board.

### 3.3 Where real use still bites

1. **An archive importer cannot preserve session start times.** The corpus carries
   `started_at`; the API has no knob for it (`WriteTxn::ensure_session(session_id, source,
   model)`; the adapter's `ensure_session` takes the same three params). All 200 imported
   sessions came back with `started_at=1792510163.87` — the *import* wall-clock — and
   `ended_at` likewise, so a session's `started_at` can be **later than the timestamps of
   its own messages** (message `ts=1760900000`, session `started_at=1792510163`). The
   store's `idx_sessions_effective_activity` and upstream's `after_ts`/`before_ts` routes
   bound `sessions.started_at` (spec §2.3/§2.4), so time-window queries over an archive are
   meaningless. `specs/persistence-shim.md` §6.1 writes `ensure_session(id, source, …)`
   and says nothing about times, and no doc mentions the import case. → `DF-HRS-115`.
2. **`open` silently ignores wrong-typed `create` / `read_only`.** `create: "yes"` and
   `create: 1` are treated as absent (the user gets `Io … unable to open database file`
   with no hint), and `read_only: "true"` is treated as **false** — a caller who thinks
   they opened a read-only viewer gets a write-capable handle on the archive
   (`src/bin/session_adapter.rs:502-515`, `.and_then(as_bool).unwrap_or(false)`). Proven
   live for all five variants. → `DF-HRS-114`.
3. **The per-model rollup is write-only through the documented API.** `record_model_usage`
   works and folds into the session projection, but no `ReadTxn` method — and no adapter
   method — returns `session_model_usage` rows; the repo's own test reads them with raw
   `rusqlite` SQL. A consumer building a cost report must know the table by name. →
   `DF-HRS-115` (documented, with the table/column names).
4. **The skip log can glue two records onto one line.** With the `sqlite3` CLI absent, 5
   tests skipped but the log holds **5 records in 4 lines** (two `SKIPPED:` records share
   one line: `…verification)SKIPPED: f9_…`, plus a stray empty line). Cause: `writeln!`
   through an `OpenOptions::append` file is not a single atomic write, so concurrent test
   threads interleave mid-record. → `DF-HRS-116`.
5. **The board's own events file is fused and fails validation.** `boardctl validate -C .`
   on this repo: `RESULT: FAIL (1 error(s), 18 warning(s))`, `[error] events.jsonl: line
   22: multiple JSON values on one physical line` — a second JSON event (475 bytes,
   `{"id":18,"actor":"foreman","event_type":"task_completed","task_id":"HRS-010"…`) glued
   to the previous record. Verified against the **committed** blob
   (`git show HEAD:.coding-hermes/board/events.jsonl` → 42 lines, 1 bad), so it is on HEAD,
   not working-tree residue. → `DF-HRS-117`. The practical impact is wider than a failing
   check: `boardctl event` — the board's own event writer — **refuses to write** while the
   fused record exists (it printed the same error and appended nothing), so the
   `DF-HRS-112` measurement event this run wanted to file could not land and is recorded in
   §4 of `docs/dogfood/diagnostics.md` instead.

Minor observations, deliberately **not** filed as rows: the adapter's `shutdown` exits 0
with no reply frame (spec-conformant — §4's table says "process exits 0" — but the "one
reply line per request" phrasing elsewhere is loose, now noted in the skill); `hermes-rs
--help` prints the scaffold line and exits 0 with no `--help`/`--version` (the README is
explicit that the crate has no agent functionality yet, so this is scope, not a defect).

## 4. Step 2b — what using it costs (measured, then judged)

| operation | command | number |
|---|---|---|
| cold build of a consumer from scratch (16 cores) | `CARGO_TARGET_DIR=… cargo build --release` in an empty target dir | **83 s** (rusqlite `bundled` C amalgamation dominates) |
| cold store creation (full upstream DDL + FTS scaffold) | `hrspack session <new>.db sess-x` ×3 | **1.19 / 1.22 / 1.40 s** (clean sample) · **3.0–4.4 s** (under load) — user CPU 0.01 s, so it is I/O wait on a starved host |
| warm open of a 4,000-message store | same, existing store ×3 | **0.06 s** clean · 0.14–0.18 s under load |
| import 4,000 messages | `hrspack import … batch` / `… single`, 3 runs each | batch 5.4/9.3/10.5 s · single 4.5/4.8/17.3 s — **indistinguishable**; user CPU is 1.0–1.3 s in every run (≈0.3 ms/message of CPU, the rest is commit I/O) |
| search, end-to-end CLI invocation | `hyperfine --warmup 3 --runs 10` | mean **127–169 ms** wall, **6–18 ms** user → the in-process query is 1.3–14.5 ms; the wall is process start + host contention |

**Verdict: nothing here is slow enough for a user to feel, so no `PERF-<n>` row is filed.**
Two honest caveats instead: (a) the wall numbers swing up to 4× because the host sat at
load 9–23 on 16 cores with a concurrent fresh-build battery on a bunker agent — the CPU
column is the only stable signal, and it is tiny; (b) the batch API buys **round-trip**
savings on the wire (that is what `docs/evidence/hrs109/batch-2000-wire.log`'s 11.7×
measures), **not** per-message savings inside an existing transaction — at the Rust API
level, one txn per session, `append_message` ×N and `append_messages_batch` measure the
same. That is recorded as an event on `DF-HRS-112` rather than as a new row — except that
`boardctl event` currently refuses to write against this board (§3.3 item 5), so it lives in
§4 of `docs/dogfood/diagnostics.md` and will be attachable once `DF-HRS-117` is fixed.

## 5. Install leg (ephemeral bunker)

`bunker-qa.sh launch /home/kara/hermes-rs --keep` → `LAUNCHED agent=3299f0f7
evidence=/tmp/bunker-qa-evidence-20261020T152815Z-1151160.jsonl` (server `bunker-mvp`,
ff-only pull of QA HEAD `7331204`). **Status at the end of this run: PARTIAL — running,
not graded.** Live peek over `bunker exec 3299f0f7 --server bunker-mvp`:

- cells already graded on the agent: 6, last = `toolchain-bootstrap OK` (`system gcc/g++`
  wrappers, GNU Make 4.3) with `INFO bare agent missing system deps: openssl(libssl-dev)`;
- a live `cargo` process with etime 06:01 and the repo synced to `~/qa-hermes-rs`;
- the host is at **100% disk (143 G used / 150 G, 1.1 G free)** — the documented
  host-disk class, i.e. a HARNESS shape, not a product defect.

This is **not** a silent pass and **not** a `SKIPPED-install-bunker` row: the leg provably
ran (launch + 6 graded cells + a live build). Finish it with
`bunker-qa.sh collect --evidence /tmp/bunker-qa-evidence-20261020T152815Z-1151160.jsonl`
(idempotent; pulls the per-cell rows and destroys the agent on `CELLS-DONE`; the agent's
4 h TTL keeps the evidence recoverable meanwhile).

## 6. Verdict

**PROMISING-BUT-ROUGH — leaning SHIPPABLE for the persistence slice.** The promise holds
under real use: a Rust consumer written from the docs alone packs 4,000 messages into a
faithful state.db-compatible file (byte-exact content, schema 31, integrity ok), searches
it correctly (19/20 probes exact against an independent oracle), snapshots it, and the same
file reads back through the Python adapter. Run 2's roughness is largely *repaired* —
search fidelity, the fresh-store report and backup are all fixed and verified here. What
remains is narrower and mostly about the edges a user meets when the store is used as an
*archive*: session times that no caller can set, adapter parameters that fail silently on
wrong types, a rollup nobody can read back through the documented API, plus two
artifact-hygiene defects (interleaved skip log, fused board event) and the stale usage
skill that this run corrected.

## 7. Left behind

- `docs/dogfood/2026-10-20-run3-integration.md` (this report)
- `docs/evidence/dogfood-20261020/` — 11 logs, 3 JSON result sets, the consumer crate
  source (`hrspack-main.rs.txt`, `hrspack-Cargo.toml.txt`), every probe script and the
  corpus generator (`*.py.txt`), and `run_evidence.sh.txt` to reproduce the lot
- `docs/dogfood/diagnostics.md` — run-3 section (how the slice behaves now, what the
  traps really are at HEAD, the import-time lesson)
- `skills/hermes-rs-usage/SKILL.md` — trap list rewritten from *open defects* to
  *verified-fixed, with the surviving gotchas*; archive-import notes added
- `.coding-hermes/dogfood-log.md` — run-3 entry
- Board rows `DF-HRS-113` … `DF-HRS-117` (findings only; no code touched)
