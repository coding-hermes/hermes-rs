# Persistence shim and stock-SQLite compatibility contract (HRS-002)

| Field | Value |
|---|---|
| Status | SPECIFIED (design only — no implementation, no engine selection, no performance claim) |
| Date | 2026-10-15 |
| Task | HRS-002 (Roadmap Gate 3: "Specify the persistence boundary") |
| Upstream pin | **v0.22.0** @ commit `e06df9c2eae6901f4c88f464e0ae6e753c0428b8` (tag object `84094bd903791c35867605e28e25075baa293adc`, commit dated 2026-10-13) — same pin as every HRS-001 row in `COMPATIBILITY_MATRIX.md`; provenance in `UPSTREAM_BASELINE.md` §1. All upstream file/line/symbol references below were read directly from the pinned checkout and cite `path:symbol` (line numbers where stable). |
| Companion docs | `specs/HERMES_RS_COMPATIBILITY.md` (contract §1 database requirements, AC-4), `specs/COMPATIBILITY_MATRIX.md` (row "Persistence and database access"), `specs/ROADMAP.md` (Gates 3/5/7), `specs/UPSTREAM_BASELINE.md` (pin + workload profiles), `specs/TEST_ADAPTER_CONTRACT.md` (HRS-007 fail-closed target adapter) |
| Consumers | HRS-010 (session-store vertical slice — §6), HRS-009 (optional-backend evaluation — §7, deferred) |
| Conformance check | `scripts/check_persistence_spec.py` (section presence + forbidden-claim grep; executable, passing) |

Normative language: **MUST / MUST NOT / SHOULD / MAY** as in RFC 2119. "Upstream" means
the pinned Hermes Agent v0.22.0 tree. Every upstream-behavior claim in §2 carries a
source reference; where a behavior is inferred rather than read, it is marked
`inferred`.

## 1. Purpose and scope

### DF-HRS-109 batch append implementation note

`append_messages_batch` is a correctness-first API: message rows are inserted
within the enclosing write transaction and session counters are updated once
after the batch. The stock-SQLite path therefore avoids per-row counter UPDATEs;
the Turso compatibility implementation currently delegates each message through
its existing append logic. Throughput parity with `append_message` remains the
baseline until measured evidence supports further backend-specific work.

This document specifies, as design (no code):

1. An **inventory** of the pinned upstream state store's persistence surface —
   tables, SQL operations, transaction boundaries, FTS5 use, WAL/checkpoint
   assumptions, identifiers, migrations, backup/restore, and cross-process
   behavior (§2).
2. A Rust **persistence shim API** that expresses the required semantics without
   committing callers to any storage engine — the caller-facing API contains no
   engine-specific types; the only concrete backend named by this project at
   this stage is stock SQLite (§3).
3. The **stock-SQLite compatibility contract**: standard SQLite is the
   maintained default backend, and existing upstream-produced SQLite files are
   preserved schema- and file-compatible (§4), with the fixtures that prove it
   (§5).
4. The **session-store vertical-slice contract** HRS-010 consumes (§6) and the
   evaluation/migration work explicitly **deferred to HRS-009 pending
   measurements** (§7).

Out of scope (by Roadmap Gate 3): choosing or benchmarking any alternative
engine, any speed statement, any Turso or PostgreSQL adoption decision. Each of
those is an HRS-009 deliverable gated on measured evidence; nothing in this
document may be read as such a claim. The contract's own words govern:
"Standard SQLite remains a supported backend and the compatibility baseline"
(`ROADMAP.md`, Gate 3) and "PostgreSQL is an optional deployment choice, not a
mandatory migration" (`HERMES_RS_COMPATIBILITY.md` §1).

## 2. Upstream persistence inventory (v0.22.0 @ `e06df9c2`)

Full pin: `e06df9c2eae6901f4c88f464e0ae6e753c0428b8` (see header). Every item
below carries its source reference in that tree.

### 2.1 Storage surface and file set

| Artifact | Source |
|---|---|
| `state.db` at `get_hermes_home()` — the single session/state database; path resolved fresh per handle so a runtime `HERMES_HOME` redirect works | `hermes_state.py:177` (`DEFAULT_DB_PATH`), `hermes_state.py:199-200` |
| `state.db-wal` / `state.db-shm` sidecars when the journal mode is WAL; SQLite unlinks both on a clean last close, ending the WAL "generation" | `hermes_state.py:246` (comment), `hermes_state_lockguard.py` module doc |
| `<db>.repair.lock` — offline schema-surgery admission; `<db>.fts_rebuild.lock` — runtime FTS-rebuild admission (distinct locks: surgery runs minutes in VACUUM on an EXCLUSIVE offline connection) | `hermes_state_common.py:1080-1104` (comment block), `hermes_state_repair._cross_process_repair_lock` |
| `<home>/sessions/<id>.jsonl` — append-only emergency journal for pending messages when the database was replaced under a handle | `hermes_state.py:431` |
| Per-session JSONL session files coexisting with the store (legacy/recovery surface read by salvage) | `hermes_cli/session_lost_and_found.py` (referenced by `hermes_state_ids.py` module doc) |

### 2.2 Tables (`SCHEMA_SQL`, `hermes_state_common.py:373-610`)

Schema version constant: `SCHEMA_VERSION = 31` (`hermes_state_common.py:283`).
All DDL is `CREATE … IF NOT EXISTS` and idempotent.

| Table | Role (source: `hermes_state_common.py` unless noted) |
|---|---|
| `schema_version(version INTEGER)` | single-row version marker (`hermes_state_schema.py:988-990` reads/inserts it) |
| `system_prompts(hash TEXT PK, prompt)` | deduplicated system prompts; `sessions.system_prompt_hash` FK |
| `sessions` | ~60 columns: `id TEXT PK`, `source`, `session_key`, `chat_id`/`chat_type`/`thread_id`/`user_id` (gateway routing origin), `model`, `model_config` (JSON text), `system_prompt_hash` FK, `parent_session_id` FK (compression chains), `started_at REAL NOT NULL`, `ended_at REAL`, `end_reason`, per-session counters (`message_count`, `tool_call_count`, `api_call_count`), token counters (input/output/cache_read/cache_write/reasoning), billing columns (`billing_provider`, `billing_base_url`, `billing_mode`), cost columns (`estimated_cost_usd`, `actual_cost_usd`, `cost_status`, `cost_source`, `pricing_version`), compression bookkeeping columns, `handoff_state` columns, `rewind_count`, `archived`/`auto_archived`/`pinned`/`hidden`, `last_activity_at` + provenance, `tool_names`, `expiry_finalized` |
| `messages` | `id INTEGER PRIMARY KEY AUTOINCREMENT`, `session_id TEXT NOT NULL REFERENCES sessions(id)`, `role`, `content`, `tool_calls` (JSON text), `tool_call_id`, `tool_name`, `timestamp REAL NOT NULL`, token/finish/reasoning columns, `platform_message_id`, `observed`, `_compressed_summary`, `active`, `compacted`, `api_content` (byte-fidelity sidecar), display columns (`display_kind`, `display_metadata`, `display_identity BLOB`, `display_order`), logical-identity columns (`message_uid`, `absorbed_message_uids`, `tool_call_uids`, `tool_call_uid`) |
| `session_model_usage` | per `(session_id, model, billing_provider, billing_base_url, billing_mode, task)` usage/cost rollup; `ON DELETE CASCADE` from sessions |
| `state_meta(key TEXT PK, value TEXT)` | markers: FTS rebuild bookkeeping, staleness flags (§2.6), `fts_storage_version` |
| `gateway_routing(scope, session_key PK, entry_json, updated_at)` | prompt-cache affinity routing entries |
| `gateway_hygiene_state(session_key PK, failure_streak)` | gateway failure bookkeeping |
| `conversation_generations(source, session_key PK, generation)` | monotonic per-routing-peer generation counter; deliberately **never garbage-collected** — collecting one row re-issues a retired affinity identity (ABA), so `delete_session`/prune MUST NOT cascade into it (long comment, `hermes_state_common.py:544-566`) |
| `gateway_heartbeats(backend_id PK, pid, started_at, last_heartbeat, profile, host)` | multi-backend liveness; the startup orphan reap consults it before reaping sessions (`hermes_state_common.py:559-566`) |
| `compression_locks(session_id PK, holder, acquired_at, expires_at)` | cross-process compression lease |
| `session_turn_leases(conversation_id PK, holder, acquired_at, expires_at)` | per-conversation turn lease (TTL default 300 s, `hermes_state_messages.py:389-412` signature) |
| `async_delegations` | delegation records incl. delivery outbox columns (`delivery_state`, `delivery_attempts`, `delivery_claim`…); column set is kept byte-identical to the delegation tool's own DDL to avoid schema drift between fresh installs and lazy-ALTER installs (#94691, comment at `hermes_state_common.py:601`) |

### 2.3 Indexes, views, triggers

- ~25 named indexes in `SCHEMA_SQL` + `DEFERRED_INDEX_SQL`
  (`hermes_state_common.py:612-745`). Notable shapes the Rust backend MUST
  tolerate because upstream creates them:
  - **partial indexes**: `idx_messages_assistant_calls_by_session … WHERE role
    = 'assistant' AND tool_calls IS NOT NULL`; `idx_messages_display_page …
    WHERE active = 1 OR compacted = 1`; `idx_messages_active_null ON
    messages(active) WHERE active IS NULL`; `idx_messages_session_uid … WHERE
    active = 1`;
  - an **expression index**: `idx_sessions_effective_activity ON
    sessions(COALESCE(last_activity_at, started_at) DESC, started_at DESC)`;
  - `DEFERRED_INDEX_SQL` deliberately runs **after** column reconciliation so
    legacy databases (missing later columns) still initialize
    (`hermes_state_common.py:610-612`, `hermes_state_schema.py` `_reconcile_columns`).
- **Views**: `messages_fts_src`, `messages_fts_trigram_src`,
  `messages_fts_cjk_src` — external-content projections shared by the FTS5
  virtual tables, their sync triggers, and backfill so the index boundary can
  never disagree with itself (`hermes_state_common.py:783-792`, `:899-908`;
  `hermes_state_fts.py:46-55`).
- **Triggers** (all `CREATE TRIGGER IF NOT EXISTS`, several
  `DROP TRIGGER IF EXISTS` + re-create pairs in `DEFERRED_INDEX_SQL` so a
  changed body reinstalls):
  - `messages_message_uid_insert` — backfills NULL `message_uid` with
    `lower(hex(randomblob(16)))` (pre-column builds bind NULL);
    `hermes_state_common.py:648-660`.
  - `messages_display_order_insert`, `messages_display_visibility_update`,
    `messages_display_identity_update`, `messages_display_identity_delete` —
    display-order maintenance over the `(active = 1 OR compacted = 1)`
    projection (`hermes_state_common.py:657-721`).
  - FTS sync triggers per index (`*_insert` / `*_delete` / `*_update`) with
    WHEN gates described in §2.6.

### 2.4 SQL operation inventory

Session lifecycle — `hermes_state_sessions.py`:
`create_session` (`:459`, upsert returning the id), `ensure_session`,
`set_expiry_finalized`, `find_session_by_origin` (`:477` — most-recent live
session per platform+chat(+thread), exact-`user_id` preference,
multi-user-ambiguity → None), `end_session` (`:520` — `ended_at IS NULL`
predicate so the **first end_reason wins**; the conversation generation is
bumped only when this call's UPDATE actually changed a row, `_end_and_bump`
`:538`), `reopen_session`/compression-chain reopens (`hermes_state_compression.py:111-182`),
`update_session_cwd`/`_meta`/`_model`/`_tool_names`/`_runtime_lock`
(`:610-894`), `get_session` (`:895`), `list_sessions_rich` (`:1377`),
`get_session_delete_targets` (`:1675`), `delete_session` (`:1719`),
`delete_session_if_empty` (`:1781`), `delete_sessions` (`:1813`).

Messages — `hermes_state_messages.py`: `append_message` (`:389` — write-guard
check inside the transaction, INSERT, counter bump, returns the row id;
runs at transcript write patience), `append_delegation_delivery` (`:423`),
`append_messages_batch` (`:458`, one transaction for the batch),
`get_messages` (`:1392` — display-generation dedupe `_dedupe_display_generations`
`:1206` collapses compaction generations so each logical message appears once),
`get_messages_around` (`:1452`), `get_messages_as_conversation` (`:1520`,
`include_ancestors` walks `parent_session_id` chains via
`_walk_compression_chain`, `hermes_state_compression.py:89`).

Search — `hermes_state_search.py`: `search_messages` (`:1063`) →
`_search_messages_impl` (`:1096`): FTS5 `MATCH` against `messages_fts` /
trigram / cjk routes; `_sanitize_fts5_query` (`:818`) escapes user input;
default sort is BM25 rank, `"newest"/"oldest"` by timestamp; zero-result Latin
queries get a fuzzy retry; CJK terms too short for trigram route to LIKE;
`after_ts`/`before_ts` bound `sessions.started_at` on every route. Slow-search
logging threshold `HERMES_SEARCH_SLOW_MS`.

Usage/cost — `hermes_state_usage.py`: `session_model_usage` upserts and
`update_session_billing_route` (`:85`); session-level counters updated by the
message append path.

Compression/delegation — `hermes_state_compression.py`:
`_claim_lease_row` (`:61`, predicated INSERT/UPDATE lease claim),
`publish_compression_child` (`:274` — parent end-stamp + child row publication
in one transaction after re-verifying the lease), generation-counter bump
(`hermes_state_messages.py:46-123`).

Bulk operations — `hermes_state_maintenance.py` (`MaintenanceMixin` `:111`):
`prune_empty_ghost_sessions`, `sweep_orphaned_sessions`, prune/archive
dry-runs (`list_prune_candidates` `:262`, `count_prune_matches` `:274`),
pinned rows excluded unless opted in (`:239`), VACUUM policy gated by
`AUTO_VACUUM_MIN_FREELIST_RATIO`. `sessions.auto_prune` defaults on upstream
(90-day inactivity; `hermes_cli/config_defaults.py:515`, release notes in
`UPSTREAM_BASELINE.md` §1).

SQL-level constraints every implementation inherits:

- `IN (?,?,…)` lists are chunked at **900** parameters
  (`_SQL_IN_CHUNK`, `hermes_state_common.py:369-373`) because
  `SQLITE_MAX_VARIABLE_NUMBER` is 999 on older builds — a bulk prune binding
  tens of thousands of ids died with "too many SQL variables".
- Foreign keys are enforced on connections
  (`PRAGMA foreign_keys=ON`, `hermes_state.py:806`).
- Timestamps are REAL epoch seconds throughout; ids are TEXT (§2.8).

### 2.5 Transaction boundaries

Writer connection: `sqlite3.connect(..., check_same_thread=False, timeout=1.0,
isolation_level=None)` — autocommit mode, so **every write transaction is an
explicit `BEGIN IMMEDIATE`** (`hermes_state.py:790-792`). All mutations flow
through one helper:

`SessionDB._execute_write` (`hermes_state.py:994-1075`) — normative semantics:

1. `BEGIN IMMEDIATE` **first** — the WAL write lock is taken up front so
   contention surfaces immediately, not at commit.
2. On locked/busy: release the in-process lock, sleep with jitter, retry the
   **whole callback** until a patience deadline. Default patiences: write
   20 s, transcript append 60 s, activity stamp 0.5 s
   (`hermes_state.py:483`); read-side busy timeout 5 s
   (`hermes_state.py:194`, `_READ_BUSY_TIMEOUT_S`).
3. The callback MUST be **idempotent under retry** (documented on the
   helper); the store owns commit; any exception rolls back.
4. `SQLITE_IOERR` raised by `BEGIN IMMEDIATE` itself is retried exactly once
   — the callback has not run, so replay is exactly-once safe. Once the
   callback has started, an IOERR leaves settlement **unknown** and MUST
   propagate (no blind retry of a possibly-applied write).
5. A live foreign compression lock is a **transient** condition with its own
   5 s wait (`_COMPRESSION_BUSY_WAIT_S`, `hermes_state.py:495`): a steer
   landing mid-compression must not abort the turn (#75083).
6. Write guards (compression lock holder, turn lease holder/TTL) are checked
   **inside** the transaction (`_check_transcript_write_guards`,
   `hermes_state_messages.py:413-421`).
7. The database-replaced / WAL-generation probe runs **inside** the process
   lock (a lock-free probe can misclassify the handle's own clean close as an
   externally deleted generation and permanently wedge writes, #105567).
8. After a successful commit, best-effort periodic side effects: a PASSIVE WAL
   checkpoint every 50 writes (`_CHECKPOINT_EVERY_N_WRITES = 50`,
   `hermes_state.py:500`) and an incremental FTS merge.

Reads: pooled read-only connections (`mode=ro`) with per-file permit ceiling
8, per-process ceiling 24, and a 64-descriptor headroom reserve
(`_READ_POOL_MAX` / `_READ_POOL_PROCESS_MAX` / `_FD_HEADROOM_RESERVE`,
`hermes_state_readpool.py:23-52`); a reader that cannot get a permit
**degrades to the writer connection** instead of queueing (EMFILE is worse
than slow). Read-only opens retry bounded `SQLITE_IOERR` during WAL
transitions (#100436, `hermes_state_readpool.py:14-22`). A busy read waits up
to 5 s. One writer connection per handle, guarded by an in-process mutex;
an async token-writer thread batches token-count updates
(`hermes_state.py:656-662` init block).

### 2.6 FTS5 use

Three FTS5 indexes over `messages`, all **external-content** (content lives in
the base tables; the index stores tokens only) as of FTS storage layout v23:

| Index | Tokenizer | Content source | Exclusions |
|---|---|---|---|
| `messages_fts` | default unicode61 | view `messages_fts_src`: tool-role rows indexed as a bounded content prefix (`substr(content, 1, FTS_TOOL_CONTENT_PREFIX_CHARS)`), else full content + `tool_name` + `tool_calls` | — |
| `messages_fts_trigram` | `trigram` | view `messages_fts_trigram_src`: joins sessions, excludes `role='tool'`, sources `cron`/`subagent`, and `_delegate_from`-marked children (`FTS_TRIGRAM_EXCLUDED_SOURCES` + `fts_trigram_session_sql`, `hermes_state_common.py:880-912`) | trigram is ~2.6× the size of the text it covers; machine-noise classes excluded deliberately |
| `messages_fts_cjk` | loadable `cjk_unicode61` (contributed bigram wrapper, PR #65544) | view `messages_fts_cjk_src` (non-tool rows) | dropped + `fts_cjk_stale` when the tokenizer extension is unavailable (`hermes_state_common.py:943-947`, `hermes_state_fts.py:30-40`) |

Semantics the shim MUST preserve (on any backend that offers search at all):

- **Deferred rebuild bookkeeping** (`hermes_state_common.py:707-740`): while a
  background rebuild is pending, `state_meta` keys `fts_rebuild_high_water`
  (H) and `fts_rebuild_progress` (P) define membership — a row is indexed iff
  `id <= P` or `id > H`; every sync trigger gates on that predicate because an
  external-content `delete` for a row the index never held **corrupts** the
  index. With no rebuild pending the predicate is a tautology.
- **Staleness markers**: `fts_stale`, `fts_cjk_stale`,
  `fts_rebuild_deferral` in `state_meta` — a stale index must keep its
  triggers DROPPED and must not serve reads until rebuilt
  (`hermes_state_common.py:943-955`).
- **Update gating**: FTS triggers use `AFTER UPDATE OF <content columns>` +
  content-diff WHEN clauses so non-content writes do not saturate FTS I/O on
  large stores (#68858/#73639).
- **Rebuild admission**: a full structural rebuild (FTS5 `'rebuild'` or
  drop/recreate) runs in ONE process at a time under `<db>.fts_rebuild.lock`
  (flock/msvcrt, 120 s bounded wait, poll 0.1 s, **fail closed**; orphaned-fd
  holders broken only when provably dead — pid + start-ticks + PID-namespace
  record under the lock; concurrent rebuilds corrupted stores in production,
  #89293/#90950/PR #93200). The chunked backfill (`fts_rebuild_step`) is
  deliberately NOT routed through the lock — it claims progress under normal
  transaction authority and is intentionally multi-process
  (`hermes_state_common.py:1046-1104`).
- **Availability probing**: FTS5 support is detected by creating a temp
  virtual table (`CREATE VIRTUAL TABLE temp._hermes_fts5_probe USING
  fts5(x)`, `hermes_state_schema.py:214`); trigram/tokenizer availability by
  probe table (`hermes_state_schema.py:695`). A tokenizer-less process keeps
  writes alive by dropping the cjk triggers and flagging staleness.
- **Merge tuning**: `usermerge` is lowered to its minimum 2 (persisted in the
  `%_config` table) during rebuilds on large databases
  (`hermes_state_search.py:1345-1367`).
- **Legacy layout coexistence**: pre-v23 installs keep inline-content FTS
  tables (`LEGACY_FTS_SQL` / `LEGACY_FTS_TRIGRAM_SQL`,
  `hermes_state_common.py:958-1044`) until the user opts into
  `hermes db optimize` / `hermes sessions optimize-storage`
  (`hermes_cli/sessions_cmd.py:922`, `optimize_fts_storage`). Fresh installs
  are born external-content; the legacy DDL is never applied to a fresh file.

### 2.7 WAL / journal-mode / PRAGMA policy

Source: `hermes_state_wal.py` unless noted.

- Journal mode is **configured, default `wal`**, with a `delete` fallback for
  filesystems where WAL is unsafe (`resolve_journal_mode`, `:157-165`).
  Setting WAL is verified by reading back the mode; refusal paths exist for
  cross-VM filesystems (virtiofs/9p → `WalUnsupportedError` → DELETE,
  `:314-339`) and macOS NFS/SMB quiesce windows. A non-WAL switch may only be
  issued with `busy_timeout=0` so SQLite's exclusivity doesn't wedge the
  switch (`_set_journal_mode_no_wait`, `:397-415`).
- `PRAGMA journal_size_limit = 64 MiB` (`_WAL_SIZE_LIMIT_BYTES`, `:31`, applied `:91`).
- macOS-only hardening: `synchronous=FULL` and `checkpoint_fullfsync=1`
  enforced on WAL connections (`:104-121`) — Apple fsync guarantees neither
  data-on-platter nor checkpoint ordering.
- Caller-configurable PRAGMAs (`hermes_state_wal.py:625-635`):
  `cache_size`, `mmap_size`, `temp_store`, `wal_autocheckpoint`,
  `journal_size_limit`, `synchronous`. Journal mode itself is NOT
  caller-settable here (owned by the resolver).
- **Checkpoints: PASSIVE only at runtime.** `wal_checkpoint(TRUNCATE)`
  corrupted B-trees on 65K+-page databases under exclusive-lock I/O pressure,
  so runtime code uses `PRAGMA wal_checkpoint(PASSIVE)` exclusively
  (`hermes_state.py:1471-1484`, `:1550-1557`); TRUNCATE belongs to explicit
  offline maintenance only. Close-time checkpoints are avoided on
  lost-generation handles (a full WAL reset races a live writer, `:1509`).
- Foreign keys ON; writer `timeout=1.0` (SQLite-internal busy wait) with the
  application-level retry owning real patience (§2.5).

### 2.8 Identifiers

Source: `hermes_state_ids.py` (module doc: "the ONE place that knows the
`YYYYMMDD_HHMMSS_<hex>` shape").

- `new_session_id` (`:56`): `%Y%m%d_%H%M%S_<hex>`; interactive default hex
  length 6, gateway keys 8, portability imports 12. The timestamp may be
  caller-pinned so the id and row agree to the second.
- **Recognizer set** (`SESSION_ID_RECOGNIZERS`, `:18-40`): interactive prefix
  `^\d{8}_\d{6}_`; `cron_<job>_<ts>`; `bg_(\d{6}_)?hex6`; `room_<sha256[:32]>`;
  `api_<ts>_<hex8>`; `api-<hex16>`; `run_<uuid4hex>`; bare UUIDv4 strings.
  Salvage classifies schema-less rows with this set, so a shape change is a
  recovery-classification change.
- `message_uid`: minted as `lower(hex(randomblob(16)))` when absent — by the
  store (`stamp_message_uid`) or by the insert trigger for rows written by
  pre-column builds (§2.3). Durable per logical message; `tool_call_uids`
  analogously.
- `messages.id` is `INTEGER PRIMARY KEY AUTOINCREMENT`: rowids are **durable
  message ids** (never reused after delete), which the FTS membership
  arithmetic and `get_messages_around` both rely on.
- `conversation_generations` is a monotonic, never-GC'd counter per
  `(source, session_key)` (§2.2) — the generation for a peer only ever
  increments, and only in the transaction that actually writes a boundary.

### 2.9 Migrations

Source: `hermes_state_schema.py` unless noted.

- `schema_version` single-row table; current `SCHEMA_VERSION = 31`
  (`hermes_state_common.py:283`). Read + insert-if-absent at init
  (`:988-990`); advanced at the **end** of the migration pass.
- **Column reconciliation is not a versioned migration**: missing columns are
  reconciled additively on every open (`_reconcile_columns`); "column adds
  never skip; schema_version remains for data migrations only" (`:925`).
- Deferred indexes and trigger reinstall pairs run after reconciliation
  (§2.3) so legacy databases initialize.
- **FTS storage layout is versioned independently** via the `fts_storage_version`
  state_meta marker (`:1086-1087`): the v22→v23 inline→external-content switch
  is opt-in (`optimize_fts_storage`), and `schema_version` advances regardless
  (`:1107-1111`).
- Point-repairs that are part of normal init: `session_model_usage` heal DDL
  for pre-v22 stores missing the `task` dimension
  (`_SESSION_MODEL_USAGE_HEAL_DDL`, `:77`); legacy `gateway_routing` rebuild
  (`:852`); trigger-body drift repaired via DROP+CREATE IF NOT EXISTS with a
  settled-body skip (`:306-326`).
- **Malformed `sqlite_master`** (typically a duplicate
  `CREATE VIRTUAL TABLE messages_fts` row) makes every statement raise; init
  performs one in-place `writable_schema` repair of the FTS layer
  (`hermes_state_errors.py:6-17`; `hermes_state.py:706-708`).
- FTS5/tokenizer availability probes gate migration steps that need them
  (§2.6); a store missing FTS5 support still opens (search degrades, writes
  continue with stale markers).

### 2.10 Backup / restore / recovery

- **Snapshot = SQLite backup API.** `hermes_cli/backup_sqlite.py`
  `_safe_copy_db` (`:26-60`): read-only source URI, destination pre-created
  `0600` with `O_NOFOLLOW`, `timeout=0.0` (the progress callback owns the
  busy deadline), fail closed — "copying only the main file loses WAL data".
  `hermes_cli/backup.py` pairs the snapshot with the captured `-wal`
  (`:120-133`). Restore is a page-copy publish with a foreign-holder scan and
  atomic replace (`hermes_cli/backup_restore.py:1-5`).
- **Retired-generation capture**: `capture_retired_wal_generation`
  (`hermes_state_dbfile.py:514`) copies the whole main image (under a size
  threshold, `:409-412`) plus `-wal`/`-shm` into a staging dir with a
  manifest; above the threshold only the 100-byte header is kept. The
  artifact is a self-contained pair recoverable via `hermes sessions recover
  --source`. Short reads abort the capture (a truncated copy with a
  valid-looking header is a trap).
- **Quarantine**: a zeroed/clobbered `state.db` (0-byte or non-SQLite page 0,
  detected by a 100-byte pre-open probe — `is_zeroed_state_db`,
  `has_invalid_sqlite_header_preopen`, `hermes_state_dbfile.py:636-648`) is
  moved aside **with its sidecars** under a cross-process lock
  (`quarantine_invalid_state_db`, `:696`) before a fresh store is scaffolded.
- **Generation discipline**: a handle whose database file was replaced
  out-of-band raises `StateDbReplacedError`; a handle stranded on an unlinked
  WAL generation raises `DeletedWalGenerationError`
  (`refuse_deleted_wal_generation`, `hermes_state_dbfile.py:385`) and its
  connection is retired unclosed rather than closed (close would
  checkpoint/unlink a live sibling's WAL).
- **VACUUM/retention**: `hermes_state_maintenance.py` prune/archive policies;
  offline `optimize-storage` compacts FTS storage with the gateway stopped
  (`hermes_cli/doctor_state.py:95-103`); schema surgery runs under
  `<db>.repair.lock` on an EXCLUSIVE offline connection (minutes in VACUUM).

### 2.11 Cross-process behavior

One `state.db` is routinely shared by several live processes (gateway
service, Desktop `hermes serve` backend, interactive CLI, TUI slash worker —
`hermes_state_common.py:1046-1050`). Consequences the shim MUST respect:

- **WAL generation protection**: SQLite protects a live WAL generation with
  POSIX advisory locks; a stray `open()`/`close()` of `-wal`/`-shm` inside
  the holder process cancels them (sqlite.org/howtocorrupt §2.2) and strands
  the holder. Upstream re-arms the same ranges as open-file-description
  (`F_OFD_SETLK`) locks on SQLite's own descriptors — main-file shared range
  at `0x40000000+2` (510 bytes) and the `-shm` DMS byte 128 — so a foreign
  close-time unlink is refused; no-op on Windows; released before the
  handle's own close (`hermes_state_lockguard.py` module doc, `:35-40`).
- **File identity probes**: inode + `application_id` + sidecar identity are
  recorded at open and compared on every write, so an out-of-band replace
  (`mv` or `cp`) cannot limp through in-place surgery
  (`hermes_state.py:607-618`).
- **Lock-holder liveness is PID-namespace-aware**: `/proc/<pid>/stat` field
  22 start-ticks + recorded `pidns`; anything indeterminate fails closed and
  defers (`hermes_state_pidns.py`, `hermes_state_common.py:1106-1160`).
- **Liveness heartbeats**: backends register in `gateway_heartbeats` and the
  startup orphan reap spares sessions whose backend is alive-but-idle
  (§2.2, `hermes_state_common.py:559-566`).
- **Handle registry**: `hermes_state_registry.acquire()` refcounts shared
  handles; `close()` on a registry-owned handle is a no-op — the registry
  owns the lifecycle (#90837).
- **Advisory-lock classification**: flock → `EWOULDBLOCK`/`EAGAIN`,
  msvcrt → `EACCES`/`EDEADLK` are *contention* (retry); `ESTALE`, `ENOTSUP`,
  `ENOLCK`, `EIO` are persistent failures (fail closed now)
  (`hermes_state_common.py:1101-1110`).
- **Test isolation**: pytest-context processes are refused the production
  `state.db` (env `HERMES_STATE_DB_GUARD_BYPASS` for sanctioned use;
  `hermes_state_guard.py`).

### 2.12 Upstream conformance cells (crash/resume)

`tests/conformance/persistence/` (README + `_harness.py`) defines
deterministic, LLM-free cells run against the real `SessionDB` with **real
SIGKILL** to a separate OS process mid-write: cell 1 prefix durability
(≥200-append kill window, contiguous prefix, journal-mode matrix including an
explicit DELETE leg with on-disk mode audit), cell 2 consume-once (8-process
file-barrier race over one predicated UPDATE), cell 3 rotation atomicity
(compression end + child publication all-or-nothing), cell 4 fork determinism
(**stub**, interlocked with #82956–#82959), cell 5 delivery-outbox
exactly-once across reboot. The HRS-007 manifest classes these
`platform-host` (13 cases) — they need a real OS process and are therefore
subject to environment skip-with-reason in CI, never silent removal.

## 3. Rust persistence shim API (engine-neutral)

### 3.1 Layering and the engine-leak rule

Callers (session store, gateway, CLI layers; HRS-010 first) depend only on the
shim crate's traits and plain data types. **The caller-facing API MUST NOT
contain types, error variants, connection strings, or configuration keys
specific to any candidate engine.** The only concrete backend this project
names at this stage is stock SQLite, and it appears only in its own backend
module's constructor (e.g. `stock_sqlite::SqliteBackend::open(path)`), never
in shared signatures. Whether any other backend module ever exists is an
HRS-009 outcome, not an API assumption: capability discovery (§3.2) is the
only mechanism callers use to learn what a backend supports.

### 3.2 Backend selection and capability query

```rust
// Illustrative shape — the implementing RFC owns exact signatures.
pub trait PersistenceBackend: Send + Sync {
    /// Open (creating the file only when allowed) and report what happened.
    fn open(&self, opts: OpenOptions) -> Result<OpenStore, PersistenceError>;
}

pub struct OpenStore {
    pub store: Box<dyn PersistenceStore>,
    /// What this backend/filesystem combination actually supports, after
    /// probing — never what the caller asked for.
    pub capabilities: Capabilities,
    /// Schema state after open: migrations applied, versions, degraded flags.
    pub report: OpenReport,
}
```

Capabilities are a plain data struct queried by callers — there is no
engine-name check anywhere in caller code:

```rust
pub struct Capabilities {
    pub journal: JournalCapability,          // { Wal | RollbackOnly | Configurable }
    pub fts: FtsCapabilities,                // base / trigram / cjk, each
                                             // Available | Unavailable { reason }
    pub checkpoint: CheckpointCapability,    // { PassiveOnly }
    pub advisory_generation_guard: bool,     // OFD-lock WAL-generation guard
    pub file_format: FileFormatCapability,   // { StandardSqliteFile } when the
                                             // engine reads/writes stock files
    pub bulk_parameter_limit: u32,           // e.g. 999 vs 32766
}
impl Capabilities {
    pub fn supports(&self, c: Capability) -> CapabilitySupport; // Yes / No{reason} / Degraded{reason}
}
```

Rules:

- A caller MUST branch on `capabilities` (or receive an explicit
  `PersistenceError::Unsupported` when it calls an operation the capability
  query already ruled out). Silent degradation is forbidden — the upstream
  store marks degradation in `state_meta` and refuses stale reads (§2.6);
  the shim expresses the same through capability flags + `Degraded` answers.
- `OpenOptions` carries the upstream-equivalent knobs that exist at the
  contract level (journal-mode preference `Wal | RollbackOnly`,
  create-if-missing, read-only) and nothing engine-private.

### 3.3 Transaction semantics

Normative semantics (mirroring §2.5; the sketch is non-normative):

```rust
pub trait PersistenceStore: Send + Sync {
    /// One write transaction. `f` receives a plain `WriteTxn` handle exposing
    /// parameterized statements for the operation set (§6) — no engine types.
    /// Commit is owned by the store. `f` MUST be idempotent under retry.
    fn write(&self, patience: Patience, f: &mut dyn FnMut(&mut WriteTxn) -> Result<T, StoreError>)
        -> Result<T, PersistenceError>;
    fn read(&self, f: &mut dyn FnMut(&ReadTxn) -> Result<T, StoreError>)
        -> Result<T, PersistenceError>;
}
```

1. Write transactions acquire the writer lock **first** (upstream:
   `BEGIN IMMEDIATE`), so contention surfaces at entry.
2. Locked/busy contention is retried with jitter inside the patience budget
   (upstream defaults: 20 s generic / 60 s transcript / 0.5 s activity / 5 s
   compression-busy — §2.5); the callback may be replayed whole, hence the
   idempotence contract.
3. Commit is owned by the store; every error path rolls back before
   returning.
4. An engine-level I/O error **at transaction entry** is retryable-once
   (nothing ran). An engine-level I/O error **after the callback started**
   leaves settlement unknown: the shim returns
   `PersistenceError::UnknownOutcome` and MUST NOT auto-retry — the caller
   re-runs the idempotent operation after checking state.
5. Lease-style guards (compression lock, turn lease) are checked inside the
   transaction and surfaced as typed lease errors (§3.4), with the upstream
   TTL semantics (default 300 s) and expiry-based reclaim.
6. Reads are snapshot-consistent and do not block the writer (WAL readers),
   subject to capability `journal`; a capability-limited backend says so via
   `Capabilities::supports`.

### 3.4 Error taxonomy

One enum, mapped 1:1 to the upstream cause tuple
(`PERSISTENCE_ERROR_CAUSES`, `hermes_state_errors.py:137-140`):
`locked, compression, compression_closed, turn_lease, corrupt, fts_index,
replaced, deleted_wal, disk, session_row_missing, unknown`.

```rust
pub enum PersistenceError {
    Busy { kind: BusyKind },                 // SQLITE_BUSY/LOCKED analog; retryable
    LeaseLost { lease: LeaseKind },          // compression lock / turn lease
    Corrupt { scope: CorruptScope },         // Store | MalformedSchema | FtsIndex
    DiskFull,                                // ENOSPC / SQLITE_FULL analog
    Replaced,                                // file identity changed under the handle
    WalGenerationLost,                       // stranded on unlinked sidecars
    SessionRowMissing,
    FtsUnavailable { reason: String },
    Schema { need: SchemaNeed },             // unknown/newer version, migration required
    Unsupported { capability: Capability },  // capability query already ruled this out
    UnknownOutcome,                          // §3.3(4): settlement unknown, caller re-checks
    Io(std::io::Error),
}
```

Classification rules ported from `hermes_state_errors.py`:

- Lock errors are classified **by result code first** (`SQLITE_BUSY` /
  `SQLITE_LOCKED`), text only as fallback — FTS5's xConnect race replaces
  BUSY with "vtable constructor failed" prose while keeping the code.
- Corruption carries a **scope**: `SQLITE_CORRUPT_VTAB` (267) means
  FTS-scoped even with generic text; bare corrupt/not-a-database codes are
  store-scoped. FTS-scoped corruption is repairable at the index layer;
  store-scoped corruption is a sticky quarantine (`StateDbCorruptError`
  semantics), never retried into a timeout.
- "Busy right now, not damaged" is one predicate feeding both retry policy
  and any HTTP 503-vs-500 split; corruption is deliberately absent from the
  transient set.
- Disk-full (`ENOSPC`, "database or disk is full") is its own variant — it
  MUST NOT be classified as corruption even though "disk" appears in
  corruption prose (upstream orders the checks for exactly this reason).

### 3.5 Cancellation

- Cancellation is **cooperative and boundary-observed**: a cancelled
  operation returns at the next transaction boundary. A transaction that has
  begun either commits whole or rolls back whole; partial application is
  impossible by construction (commit owned by the store).
- Cancelling after the write callback started yields
  `PersistenceError::UnknownOutcome` for the same reason as §3.3(4); callers
  recover by re-running the idempotent operation.
- Blocking waits (lock patience, lease TTL waits) take a cancellation token;
  cancellation during a wait returns `Busy { abandoned: true }`-style
  feedback without leaving locks held (the in-process mutex is released on
  all paths).

### 3.6 Migration handling

- `open()` runs the upstream-equivalent admission: read/insert
  `schema_version`, reconcile missing columns additively (not version-gated),
  run data migrations, re-apply deferred indexes/trigger repairs, probe FTS
  availability. The result lands in `OpenReport`:
  `{ schema_version_before, schema_version_after, fts_storage_version,
  columns_reconciled: […], migrations_applied: […], degraded: […], quarantined: Option<…> }`.
- A file written by a **newer** schema version than this build knows is
  never silently downgraded or rewritten: open fails with
  `Schema { need: Newer(upstream_version) }` and actionable detail (upstream
  fails closed the same way rather than guessing).
- Migration execution MUST be single-admission across processes (upstream:
  open-serialization quarantine lock + repair lock, §2.10-2.11); two
  processes migrating one file concurrently is a defect, not a race to win.
- The FTS storage layout version is tracked separately from the schema
  version (§2.9), and layout conversion is opt-in maintenance, never an
  implicit open-time rewrite.

### 3.7 How callers query capabilities (summary)

1. Construct a backend module object (stock SQLite today).
2. `open(opts)` → `OpenStore { capabilities, report, .. }`.
3. Before an optional-path operation (search, trigram search, CJK search,
   WAL-specific assumptions, bulk-parameter-sized batches): ask
   `capabilities.supports(Capability::…)`; handle `No { reason }` /
   `Degraded { reason }` explicitly (log + degrade like upstream's stale-FTS
   markers), or let the operation itself return
   `PersistenceError::Unsupported`.
4. Never inspect concrete backend types, engine names, or version strings to
   make behavioral decisions.

## 4. Stock-SQLite default backend compatibility contract

Standard SQLite is the **maintained default backend** and the compatibility
baseline (`HERMES_RS_COMPATIBILITY.md` §1; `ROADMAP.md` Gate 3). The stock
backend module wraps the SQLite engine the Rust build links (the exact
`rusqlite`/libsqlite3 pin is an implementation-RFC concern) and MUST satisfy:

### 4.1 File and schema compatibility (normative)

"Existing SQLite files preserved byte/schema-compatible" is defined — and
limited — precisely:

- **Schema-compatible**: the set of schema objects (tables, columns, indexes,
  views, triggers, virtual tables) in a file produced by the pinned upstream
  MUST be recognized and preserved. The Rust backend MUST NOT rename, drop,
  re-type, or reorder upstream columns; additive columns come only through
  the reconciliation path (§3.6) with upstream's own defaults.
- **File-compatible**: files written by the stock backend remain standard
  SQLite database files (single `state.db` + `-wal`/`-shm` sidecars; journal
  mode per §2.7) openable by any stock SQLite build, including the pinned
  Python reference. This is **not** a byte-identity claim: two equivalent
  writers legitimately differ in page layout and free lists. What MUST hold
  is that a file alternates between the Python reference and the Rust
  backend without conversion, migration, or repair.
- **No foreign objects**: the Rust backend MUST NOT add engine-private
  tables, views, triggers, or pragmas-persisted-state to a user database
  beyond the upstream census. Fixture F-10 (§5) pins the `sqlite_master`
  census.
- Upstream data conventions are preserved verbatim: REAL epoch-second
  timestamps, TEXT session ids of the minted shapes (§2.8),
  AUTOINCREMENT rowids, JSON-as-TEXT columns (`tool_calls`, `model_config`,
  `origin_json`, `entry_json`), and the `conversation_generations`
  never-GC rule.
- Journal/checkpoint policy parity (§2.7): default WAL with verified
  read-back and DELETE fallback; PASSIVE-only runtime checkpoints; 64 MiB
  `journal_size_limit` default; foreign keys ON.
- Identifier minting parity (§2.8): the Rust backend mints session ids,
  message uids, and tool-call uids in the exact upstream shapes and via the
  single minting path, because salvage and recovery classify by shape.
- Error-classification parity (§3.4): code-first lock classification,
  FTS-scoped vs store-scoped corruption, disk-full separation, sticky
  corrupt quarantine.
- Deferred-rebuild and staleness semantics (§2.6) apply unchanged when FTS5
  is available; when FTS5 is unavailable the backend reports
  `FtsCapabilities` accordingly and sets the upstream staleness markers
  rather than silently serving partial search results.

### 4.2 Explicitly unsupported / forbidden behavior (stock backend)

- Renaming or re-shaping upstream tables; migrating users to a
  shim-private schema.
- Runtime `wal_checkpoint(TRUNCATE)` or close-time WAL resets on live
  handles (§2.7 — corruption history).
- Silent journal-mode coercion (e.g. flipping a WAL file to DELETE without
  the configured-resolver semantics and verified read-back).
- Writing any schema object outside the upstream census (§4.1).
- Reusing `messages.id` rowids or changing id mint formats (salvage
  depends on both).
- Cascading deletes into `conversation_generations` (never-GC rule, §2.2).
- Treating a busy store as damaged (retry-vs-corrupt confusion, §3.4).
- Skipping the pre-open quarantine probe on a non-SQLite header (§2.10).

### 4.3 Non-goals for the stock backend

No performance tuning claims, no alternative-engine preparation inside the
stock module, no format extensions. Optimization work on this backend is
in scope for HRS-009's measured evaluation only.

## 5. Compatibility fixtures

Each fixture is specified by what it MUST assert; HRS-010 implements them as
Rust tests (plus adapter-visible checks per `TEST_ADAPTER_CONTRACT.md`).
Fixture artifact databases are produced **once** by the pinned Python
reference, stored with a recorded SHA-256, and regenerated only on a re-pin.

| ID | Fixture | Must assert |
|---|---|---|
| F-1 | **Schema recognition** | Open a copied v0.22.0-produced `state.db`: open succeeds; `OpenReport.schema_version == 31` (or ≤ 31 recognized); every §2.2 table and §2.3 index/view/trigger present by name; no repair side effects on a healthy file (byte-hash of file unchanged after a read-only open). |
| F-2 | **Round-trip file compatibility** | Rust writes (ensure_session → append_message → end_session) on the copied fixture; then a *stock* SQLite library (the system `sqlite3`) opens the file: `PRAGMA integrity_check` ok, rows visible with expected column values, `schema_version` unchanged, `sqlite_master` census equal to F-10's expected set. Where the pinned Python checkout is available in CI, the Python reference reopens the same file and reads the same rows. |
| F-3 | **Prefix durability under kill** | Mirrors upstream cell 1 (§2.12): N acknowledged appends from a child process that receives a real SIGKILL mid-burst; after restart the store contains a contiguous prefix of the acknowledgements; journal-mode matrix runs default + explicit DELETE legs where the environment permits, with on-disk mode audited; a leg that could not run is recorded as skipped-with-reason. Class: `platform-host`. |
| F-4 | **Journal-mode resolution** | Default resolution selects WAL on a WAL-safe filesystem and verifies by read-back; an explicit DELETE preference is honored; WAL refusal on a non-WAL filesystem surfaces `WalUnsupported`-class reporting (never a silent mode switch). Environment-gated legs skip with reason. |
| F-5 | **Checkpoint + WAL hygiene** | After ≥50 writes, a PASSIVE checkpoint ran (WAL does not grow without bound under the 64 MiB limit policy); `PRAGMA journal_mode` reads back `wal`; a second process can read concurrently during writes. |
| F-6 | **FTS sync + staleness** | With FTS5 available: append → search finds a unique token; delete/update-of-content → search stops finding it; a simulated stale index (markers set, triggers dropped) is reported via capabilities/staleness and not silently served. Deferred-rebuild bookkeeping: rows in the (P, H] gap are not served as hits. With FTS5 unavailable: capability says so; writes still succeed. |
| F-7 | **Error taxonomy mapping** | Second writer holding the lock → `Busy` (retryable, not corrupt); forced store-scoped corruption (truncated/garbage header after quarantine path) → `Corrupt { Store }` + sticky quarantine; injected ENOSPC → `DiskFull` (skip-with-reason where injection is impossible); replaced file (inode swap) → `Replaced`. |
| F-8 | **Backup / restore** | While a writer is active, a backup-API snapshot succeeds and fails closed on timeout; the restored file opens and contains acknowledged writes; restoring onto a path with a foreign holder is refused. |
| F-9 | **Identifier minting** | Session ids match the recognizer set shapes (§2.8); `message_uid` is 32 lowercase hex; first-reason-wins on double end; `conversation_generations` increments only on an actual boundary write and is never decremented by session deletion. |
| F-10 | **Schema census (negative compatibility fixture)** | After Rust writes, `sqlite_master` contains exactly the upstream object set (plus shim-additive columns through reconciliation, if any were required) — no engine-private objects, no dropped legacy FTS DDL handling for pre-v23 files (open a legacy-layout fixture and assert it still initializes without conversion). |

Fixture rules: every fixture records environment + skip reasons (nulls carry
reasons, matching the repo's DF-HRS-102 null contract in
`UPSTREAM_BASELINE.md` §6.2); a fixture that cannot run in CI states why in
the result, and is never silently removed from the denominator.

## 6. Session-store vertical slice (HRS-010 contract)

Roadmap Gate 5: "Implement the pinned Hermes session-store operations behind
HRS-002's boundary using standard SQLite. Open a copied Python-produced
database, perform supported operations, reopen it, and verify observable
results through the HRS-007 fail-closed adapter and Rust-native tests. Test
migration, recovery, backup, and rollback. Do not claim full Hermes parity
from this slice."

### 6.1 Operations in the slice

Backed by the stock SQLite backend only; each operation is one write
transaction (§3.3) unless noted:

| Operation | Upstream reference | Contract notes |
|---|---|---|
| `open(read_write \| read_only)` on a copied upstream file | §2.1, §2.9 | F-1/F-10 pass; `OpenReport` returned |
| `ensure_session(id, source, …)` (upsert) | `hermes_state_sessions.py:459` (`create_session`/`ensure_session`) | returns the id; id supplied by caller in upstream minted shape |
| `append_message(session_id, role, content, …)` | `hermes_state_messages.py:389` | guard check → insert → counter bump → returns durable rowid; FTS synced when available |
| `append_messages_batch(...)` | `hermes_state_messages.py:458` | one transaction for the whole batch |
| `get_messages(session_id, …)` | `hermes_state_messages.py:1392` | ascending rowid order; compaction-generation dedupe semantics; `active`/`compacted` inclusion flags |
| `search(query)` | `hermes_state_search.py:1063` | capability-gated (F-6); BM25 default order; sanitized MATCH syntax |
| `end_session(session_id, end_reason)` | `hermes_state_sessions.py:520` | first end_reason wins; generation bump only when a row actually changed |
| `record_model_usage(session_id, model, …)` upsert | `hermes_state_usage.py` | composite-key upsert per §2.2 |
| `close()` + `reopen` | §2.10 | acknowledged writes observable after reopen (prefix durability, F-3 at slice scale) |

### 6.2 Ordering and transaction boundaries

- Message order within a session is `messages.id` ascending; ids are
  assigned by the store inside the write transaction; counters
  (`message_count`, token totals) move in the same transaction as the
  insert — a reader never observes a message without its counter delta.
- No operation spans two transactions; no transaction spans two operations
  (batch append is one operation by definition).
- `end_session` after `append_message` is ordered by transaction commit
  order; the first end wins and later ends are no-ops (§2.4).

### 6.3 Verification path for the slice

1. Rust-native tests: F-1, F-2, F-5, F-6 (FTS5-available leg), F-9, F-10 run
   in CI; F-3/F-4/F-7's environment-dependent legs run where possible with
   recorded skips.
2. Migration/recovery/backup/rollback **functional** tests for the slice
   (per Gate 5): F-1 (recognition), F-7 (quarantine path), F-8 (backup API).
3. HRS-007 adapter: the slice is exercised through the fail-closed target
   adapter; a deliberately broken target must fail the paired run (adapter
   property already proven in `TEST_ADAPTER_CONTRACT.md` §1).
4. No parity claim beyond "the pinned session-store operations named in
   §6.1 behave as specified on stock SQLite".

### 6.4 Explicitly deferred to HRS-009 (pending measurements)

The following are **out of HRS-010's scope** and owned by the HRS-009 gate,
which evaluates candidate backends on corrected Hermes-shaped workloads:

- Any throughput/latency/CPU/RSS/storage-growth comparison between stock
  SQLite and any candidate engine (all such statements are deferred to
  HRS-009 pending measurements).
- Turso (or any other SQLite-compatible Rust engine) as an optional backend:
  evaluation deferred to HRS-009 pending measurements; adoption is not
  implied by this spec and requires workload, SQL/file-format
  compatibility, durability, and recovery evidence (contract §1).
- PostgreSQL: any PostgreSQL support decision is deferred to HRS-009 pending
  measurements; if pursued, SQLite↔PostgreSQL import/export tools and
  round-trip migration tests are HRS-009 deliverables (contract AC-4).
- Default-backend changes of any kind: none is implied by a benchmark
  result (`ROADMAP.md` Gate 7).
- Workload-profile replays (`continuing_conversation`, `tool_execution`,
  `sqlite_persistence` per `UPSTREAM_BASELINE.md` §6.2) against the Rust
  stack.

## 7. Deferred behavior (HRS-009 and later)

Consolidated list of statements this spec deliberately does NOT make:

1. No speed or efficiency statement of any kind is made; any speed
   comparison is deferred to HRS-009 pending measurements.
2. Treating Turso as a drop-in replacement is not asserted; its evaluation
   is deferred to HRS-009 pending measurements (workload, compatibility,
   durability, recovery evidence required first).
3. Any PostgreSQL support statement is deferred to HRS-009 pending
   measurements; PostgreSQL remains an optional deployment choice, not a
   mandatory migration.
4. No default-backend switch is proposed or implied.
5. No compatibility percentage, parity fraction, or readiness claim about
   the Rust implementation is made; Rust status in
   `COMPATIBILITY_MATRIX.md` remains "Specified (design)" until HRS-010's
   fixtures land.

## 8. Verification

- `python3 scripts/check_persistence_spec.py` — exits 0 when every required
  section of this document is present, the pin is cited, the inventory
  carries its source references, `COMPATIBILITY_MATRIX.md` points back here,
  and no forbidden claim (an affirmative speed / engine-drop-in / engine-
  support sentence outside a deferred-to-HRS-009 phrasing) is found.
- Repo gate (unchanged by docs-only edits, run before commit):
  `cargo fmt --all -- --check && cargo clippy --workspace --all-targets
  --all-features -- -D warnings && cargo test --workspace --all-features`.
- Matrix consistency: the "Persistence and database access" row cites this
  document and keeps the upstream tag/commit pin format used by HRS-001 rows.

## References

- `specs/HERMES_RS_COMPATIBILITY.md` §1 (database-layer requirements), AC-4
  (backend comparison gates), §3 (SQLite edge cases), §4 (non-goals).
- `specs/COMPATIBILITY_MATRIX.md` — pinned release header; row
  "Persistence and database access" (HRS-002).
- `specs/ROADMAP.md` — Gates 3 (this spec), 5 (HRS-010 slice), 7 (HRS-009
  evaluation).
- `specs/UPSTREAM_BASELINE.md` — pin provenance; §2 (harness), §6 (corrected
  workload profiles incl. `sqlite_persistence`).
- `specs/TEST_ADAPTER_CONTRACT.md` — fail-closed adapter; `platform-host`
  classification of upstream persistence conformance cells.
- Upstream (all at v0.22.0 @ `e06df9c2`): `hermes_state.py` and the
  `hermes_state_*.py` family (33 modules, ~20.6K lines — counted at the
  pin), `hermes_cli/backup_sqlite.py`, `hermes_cli/backup.py`,
  `hermes_cli/backup_restore.py`, `hermes_cli/sessions_cmd.py`,
  `hermes_cli/doctor_state.py`, `hermes_cli/config_defaults.py`,
  `tests/conformance/persistence/` (README, `_harness.py`, cells 1/2/3/5).
