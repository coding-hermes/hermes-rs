//! DDL + column manifest extracted VERBATIM from the pinned upstream
//! checkout (v0.22.0 @ e06df9c2eae6901f4c88f464e0ae6e753c0428b8) by
//! scripts/gen_upstream_ddl.py. Do not hand-edit; regenerate on re-pin.

pub const UPSTREAM_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS system_prompts (
    hash TEXT PRIMARY KEY,
    prompt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    created_source TEXT,
    user_id TEXT,
    session_key TEXT,
    chat_id TEXT,
    chat_type TEXT,
    thread_id TEXT,
    display_name TEXT,
    origin_json TEXT,
    expiry_finalized INTEGER DEFAULT 0,
    model TEXT,
    model_config TEXT,
    system_prompt TEXT,
    system_prompt_hash TEXT,
    parent_session_id TEXT,
    started_at REAL NOT NULL,
    ended_at REAL,
    end_reason TEXT,
    message_count INTEGER DEFAULT 0,
    tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    cwd TEXT,
    git_branch TEXT,
    git_repo_root TEXT,
    git_metadata_generation INTEGER NOT NULL DEFAULT 0,
    billing_provider TEXT,
    billing_base_url TEXT,
    billing_mode TEXT,
    estimated_cost_usd REAL,
    actual_cost_usd REAL,
    cost_status TEXT,
    cost_source TEXT,
    pricing_version TEXT,
    title TEXT,
    title_source TEXT,
    last_activity_at REAL,
    last_activity_description TEXT,
    last_activity_provenance TEXT,
    api_call_count INTEGER DEFAULT 0,
    handoff_state TEXT,
    handoff_platform TEXT,
    handoff_error TEXT,
    compression_failure_cooldown_until REAL,
    compression_failure_error TEXT,
    compression_fallback_streak INTEGER NOT NULL DEFAULT 0,
    compression_ineffective_count INTEGER NOT NULL DEFAULT 0,
    compression_recovery_deadline REAL,
    compression_overload_streak INTEGER NOT NULL DEFAULT 0,
    profile_name TEXT,
    transport_profile TEXT,
    rewind_count INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    auto_archived INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    hidden INTEGER NOT NULL DEFAULT 0,
    last_read_at REAL,
    tool_names TEXT,
    FOREIGN KEY (parent_session_id) REFERENCES sessions(id),
    FOREIGN KEY (system_prompt_hash) REFERENCES system_prompts(hash)
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL,
    content TEXT,
    tool_call_id TEXT,
    tool_calls TEXT,
    tool_name TEXT,
    effect_disposition TEXT,
    timestamp REAL NOT NULL,
    token_count INTEGER,
    finish_reason TEXT,
    reasoning TEXT,
    reasoning_content TEXT,
    reasoning_details TEXT,
    codex_reasoning_items TEXT,
    codex_message_items TEXT,
    platform_message_id TEXT,
    observed INTEGER DEFAULT 0,
    _compressed_summary INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 1,
    compacted INTEGER NOT NULL DEFAULT 0,
    api_content TEXT,
    display_kind TEXT,
    display_metadata TEXT,
    display_identity BLOB,
    display_order INTEGER,
    message_uid TEXT,
    absorbed_message_uids TEXT,
    tool_call_uids TEXT,
    tool_call_uid TEXT
);

CREATE TABLE IF NOT EXISTS session_model_usage (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    billing_provider TEXT NOT NULL DEFAULT '',
    billing_base_url TEXT NOT NULL DEFAULT '',
    billing_mode TEXT NOT NULL DEFAULT '',
    task TEXT NOT NULL DEFAULT '',
    api_call_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL NOT NULL DEFAULT 0,
    actual_cost_usd REAL NOT NULL DEFAULT 0,
    cost_status TEXT,
    cost_source TEXT,
    first_seen REAL,
    last_seen REAL,
    PRIMARY KEY (session_id, model, billing_provider, billing_base_url, billing_mode, task)
);

CREATE TABLE IF NOT EXISTS state_meta (
    key TEXT PRIMARY KEY,
    value TEXT
);

CREATE TABLE IF NOT EXISTS gateway_routing (
    scope TEXT NOT NULL DEFAULT '',
    session_key TEXT NOT NULL,
    entry_json TEXT NOT NULL,
    updated_at REAL NOT NULL,
    PRIMARY KEY (scope, session_key)
);

CREATE TABLE IF NOT EXISTS gateway_hygiene_state (
    session_key TEXT PRIMARY KEY,
    failure_streak INTEGER NOT NULL DEFAULT 0
);

-- Monotonic conversation generation per routing peer (#96811).
--
-- A host-declared conversation key (X-Hermes-Session-Key / build_session_key)
-- is per-CHAT and outlives any single conversation on it, so the prompt-cache
-- affinity scope derived from it must be qualified by which conversation is
-- currently live. Deriving that from the session rows themselves
-- (COUNT/MAX over _RESET_END_REASONS boundaries) cannot prove non-reuse:
-- delete_session() and bulk pruning remove ended rows, so an aggregate can
-- return a pair it already emitted and hand a new conversation a retired
-- affinity identity.
--
-- This counter lives outside prunable session history and only ever
-- increments, once per boundary actually written, so a generation can never
-- be reused for a peer even if every session row behind it is deleted.
--
-- These rows are deliberately NEVER garbage-collected, including when every
-- session row for the peer is gone. Collecting one resets that peer to "no
-- generation", so its next boundary writes generation = 1 again and re-issues
-- a gwk_ scope a retired conversation already used — exactly the ABA this
-- table exists to close. Do not add it to delete_session()'s cascade or to any
-- prune sweep. One (TEXT, TEXT, INTEGER) row per routing peer is the intended,
-- bounded cost.
CREATE TABLE IF NOT EXISTS conversation_generations (
    source TEXT NOT NULL,
    session_key TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source, session_key)
);

-- Per-backend liveness heartbeat (#94895). Each serve / tui_gateway process
-- registers a row at startup and refreshes ``last_heartbeat`` periodically.
-- The startup orphan sweep (sessions.startup_orphan_reap) consults this
-- table to avoid reaping rows whose owning backend is still alive but
-- just idle (multi-backend state.db shared by isolated serve processes).
-- A backend whose ``last_heartbeat`` is older than the heartbeat staleness
-- window is treated as dead; rows without ANY matching heartbeat fall back
-- to the original staleness predicate so legacy deployments keep working.
CREATE TABLE IF NOT EXISTS gateway_heartbeats (
    backend_id TEXT PRIMARY KEY,
    pid INTEGER NOT NULL,
    started_at REAL NOT NULL,
    last_heartbeat REAL NOT NULL,
    profile TEXT NOT NULL DEFAULT '',
    host TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS compression_locks (
    session_id TEXT PRIMARY KEY,
    holder TEXT NOT NULL,
    acquired_at REAL NOT NULL,
    expires_at REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS session_turn_leases (
    conversation_id TEXT PRIMARY KEY,
    holder TEXT NOT NULL,
    acquired_at REAL NOT NULL,
    expires_at REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS async_delegations (
    delegation_id TEXT PRIMARY KEY,
    origin_session TEXT NOT NULL,
    origin_ui_session_id TEXT NOT NULL DEFAULT '',
    parent_session_id TEXT,
    state TEXT NOT NULL,
    dispatched_at REAL NOT NULL,
    completed_at REAL,
    updated_at REAL NOT NULL,
    event_json TEXT,
    result_json TEXT,
    delivery_state TEXT NOT NULL DEFAULT 'pending',
    delivery_attempts INTEGER NOT NULL DEFAULT 0,
    delivered_at REAL,
    owner_pid INTEGER,
    owner_started_at INTEGER,
    task_json TEXT,
    delivery_claim TEXT,
    delivery_claimed_at REAL,
    -- Mirrors the delegation tool's own CREATE TABLE (tools/async_delegation.py
    -- _initialize_schema). Keeping the canonical fresh-install shape identical
    -- to the tool's avoids a silent schema drift: the tool's lazy
    -- ALTER TABLE ADD COLUMN used to be the only source of this column, so two
    -- databases at the same schema_version had different
    -- async_delegations shapes depending on whether the delegation tool had
    -- ever run, breaking rebuild/replay pipelines that reconstruct state.db
    -- from the canonical schema (#94691).
    origin_session_id TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source);
CREATE INDEX IF NOT EXISTS idx_sessions_source_id ON sessions(source, id);
CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id, id);
-- Partial index for the Insights assistant tool-call scan
-- (agent/insights.py _get_tool_usage / _get_skill_usage): those queries filter
-- messages by role='assistant' AND tool_calls IS NOT NULL, a small fraction of
-- rows on a large state.db. role and tool_calls are base columns, so this can
-- live in SCHEMA_SQL rather than DEFERRED_INDEX_SQL.
CREATE INDEX IF NOT EXISTS idx_messages_assistant_calls_by_session
    ON messages(session_id)
    WHERE role = 'assistant' AND tool_calls IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_compression_locks_expires ON compression_locks(expires_at);
CREATE INDEX IF NOT EXISTS idx_session_turn_leases_expires ON session_turn_leases(expires_at);
CREATE INDEX IF NOT EXISTS idx_session_model_usage_session ON session_model_usage(session_id);
CREATE INDEX IF NOT EXISTS idx_session_model_usage_model ON session_model_usage(model);
CREATE INDEX IF NOT EXISTS idx_async_delegations_delivery
    ON async_delegations(delivery_state, completed_at);
"#;

pub const UPSTREAM_FTS_SQL: &str = r#"
-- Stable projection the base word index reads and writes through: the view
-- computes EXACTLY what the triggers/backfill insert, so 'rebuild' and the
-- integrity checker always agree with the stored index.
CREATE VIEW IF NOT EXISTS messages_fts_src AS
    SELECT id,
           CASE WHEN role = 'tool'
                THEN substr(COALESCE(content, ''), 1, 8192)
                ELSE content END AS content,
           tool_name, tool_calls
    FROM messages;

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    tool_name,
    tool_calls,
    content='messages_fts_src',
    content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages
WHEN (new.id > COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                         WHERE key = 'fts_rebuild_high_water'), -1)
   OR new.id <= COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                          WHERE key = 'fts_rebuild_progress'), -1))
BEGIN
    INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
    VALUES (
        new.id,
        CASE WHEN new.role = 'tool'
         THEN substr(COALESCE(new.content, ''), 1, 8192)
         ELSE new.content END,
        new.tool_name,
        new.tool_calls
    );
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages
WHEN (old.id > COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                         WHERE key = 'fts_rebuild_high_water'), -1)
   OR old.id <= COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                          WHERE key = 'fts_rebuild_progress'), -1))
BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content, tool_name, tool_calls)
    VALUES (
        'delete',
        old.id,
        CASE WHEN old.role = 'tool'
         THEN substr(COALESCE(old.content, ''), 1, 8192)
         ELSE old.content END,
        old.tool_name,
        old.tool_calls
    );
END;

-- UPDATE OF skips the trigger entirely for non-content column writes
-- (status/compacted/observed/etc.), which is stronger than the WHEN gate
-- alone and avoids FTS I/O saturation on large state.db (#68858 / #73639).
CREATE TRIGGER IF NOT EXISTS messages_fts_update
AFTER UPDATE OF content, tool_name, tool_calls, role ON messages
WHEN (old.content IS NOT new.content
    OR old.tool_name IS NOT new.tool_name
    OR old.tool_calls IS NOT new.tool_calls
    OR old.role IS NOT new.role)
   AND (old.id > COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                           WHERE key = 'fts_rebuild_high_water'), -1)
     OR old.id <= COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                            WHERE key = 'fts_rebuild_progress'), -1))
BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content, tool_name, tool_calls)
    VALUES (
        'delete',
        old.id,
        CASE WHEN old.role = 'tool'
         THEN substr(COALESCE(old.content, ''), 1, 8192)
         ELSE old.content END,
        old.tool_name,
        old.tool_calls
    );
    INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
    VALUES (
        new.id,
        CASE WHEN new.role = 'tool'
         THEN substr(COALESCE(new.content, ''), 1, 8192)
         ELSE new.content END,
        new.tool_name,
        new.tool_calls
    );
END;
"#;

pub const UPSTREAM_FTS_TRIGRAM_SQL: &str = r#"
CREATE VIEW IF NOT EXISTS messages_fts_trigram_src AS
    SELECT m.id, m.role, m.content, m.tool_name
    FROM messages AS m
    JOIN sessions AS s ON s.id = m.session_id
    WHERE m.role <> 'tool' AND s.source NOT IN ('cron', 'subagent') AND json_extract((CASE WHEN json_valid(s.model_config) THEN s.model_config ELSE json_object() END), '$._delegate_from') IS NULL;

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts_trigram USING fts5(
    content,
    tool_name,
    content='messages_fts_trigram_src',
    content_rowid='id',
    tokenize='trigram'
);

CREATE TRIGGER IF NOT EXISTS messages_fts_trigram_insert AFTER INSERT ON messages
WHEN new.role <> 'tool'
   AND EXISTS (SELECT 1 FROM sessions
               WHERE id = new.session_id AND source NOT IN ('cron', 'subagent') AND json_extract((CASE WHEN json_valid(model_config) THEN model_config ELSE json_object() END), '$._delegate_from') IS NULL)
   AND (new.id > COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                           WHERE key = 'fts_rebuild_high_water'), -1)
     OR new.id <= COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                            WHERE key = 'fts_rebuild_progress'), -1))
BEGIN
    INSERT INTO messages_fts_trigram(rowid, content, tool_name)
    VALUES (new.id, new.content, new.tool_name);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_trigram_delete AFTER DELETE ON messages
WHEN old.role <> 'tool'
   AND EXISTS (SELECT 1 FROM sessions
               WHERE id = old.session_id AND source NOT IN ('cron', 'subagent') AND json_extract((CASE WHEN json_valid(model_config) THEN model_config ELSE json_object() END), '$._delegate_from') IS NULL)
   AND (old.id > COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                           WHERE key = 'fts_rebuild_high_water'), -1)
     OR old.id <= COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                            WHERE key = 'fts_rebuild_progress'), -1))
BEGIN
    INSERT INTO messages_fts_trigram(messages_fts_trigram, rowid, content, tool_name)
    VALUES ('delete', old.id, old.content, old.tool_name);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_trigram_update
AFTER UPDATE OF content, tool_name, role ON messages
WHEN (old.content IS NOT new.content
    OR old.tool_name IS NOT new.tool_name
    OR old.role IS NOT new.role)
   AND (old.id > COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                           WHERE key = 'fts_rebuild_high_water'), -1)
     OR old.id <= COALESCE((SELECT CAST(value AS INTEGER) FROM state_meta
                            WHERE key = 'fts_rebuild_progress'), -1))
BEGIN
    INSERT INTO messages_fts_trigram(messages_fts_trigram, rowid, content, tool_name)
    SELECT 'delete', old.id, old.content, old.tool_name
    WHERE old.role <> 'tool'
      AND EXISTS (SELECT 1 FROM sessions
                  WHERE id = old.session_id AND source NOT IN ('cron', 'subagent') AND json_extract((CASE WHEN json_valid(model_config) THEN model_config ELSE json_object() END), '$._delegate_from') IS NULL);
    INSERT INTO messages_fts_trigram(rowid, content, tool_name)
    SELECT new.id, new.content, new.tool_name
    WHERE new.role <> 'tool'
      AND EXISTS (SELECT 1 FROM sessions
                  WHERE id = new.session_id AND source NOT IN ('cron', 'subagent') AND json_extract((CASE WHEN json_valid(model_config) THEN model_config ELSE json_object() END), '$._delegate_from') IS NULL);
END;
"#;

pub const UPSTREAM_DEFERRED_INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_messages_session_active
    ON messages(session_id, active, timestamp);
CREATE INDEX IF NOT EXISTS idx_messages_display_page
    ON messages(session_id, display_order, active DESC, id DESC)
    WHERE active = 1 OR compacted = 1;
CREATE INDEX IF NOT EXISTS idx_messages_display_backfill
    ON messages(session_id) WHERE (display_order IS NULL OR display_identity IS NULL)
    AND (active = 1 OR compacted = 1);
CREATE INDEX IF NOT EXISTS idx_messages_display_identity
    ON messages(session_id, display_identity, display_order)
    WHERE display_identity IS NOT NULL AND (active = 1 OR compacted = 1);
DROP TRIGGER IF EXISTS messages_message_uid_insert;
CREATE TRIGGER IF NOT EXISTS messages_message_uid_insert
AFTER INSERT ON messages WHEN new.message_uid IS NULL
BEGIN
    -- Every row carries a message_uid, whoever wrote it: a build that predates the column binds NULL, so
    -- the store mints one on its behalf (the current build always binds a uid; this never fires for it).
    -- The note lives inside the body: a comment before DROP/CREATE would defeat the settled-trigger skip.
    UPDATE messages SET message_uid = lower(hex(randomblob(16))) WHERE id = new.id;
END;
DROP TRIGGER IF EXISTS messages_display_order_insert;
CREATE TRIGGER IF NOT EXISTS messages_display_order_insert
AFTER INSERT ON messages WHEN new.display_order IS NULL
BEGIN
    UPDATE messages SET display_order = COALESCE((
        SELECT display_order FROM messages
        WHERE session_id = new.session_id AND id <> new.id
          AND (active = 1 OR compacted = 1)
          AND display_identity = new.display_identity AND display_order IS NOT NULL
        ORDER BY display_order LIMIT 1
    ), new.id) WHERE id = new.id;
END;
DROP TRIGGER IF EXISTS messages_display_visibility_update;
CREATE TRIGGER IF NOT EXISTS messages_display_visibility_update
AFTER UPDATE OF active, compacted ON messages
WHEN (new.active = 1 OR new.compacted = 1) <> (old.active = 1 OR old.compacted = 1)
BEGIN
    UPDATE messages SET display_order = MIN(new.id, COALESCE((
        SELECT display_order FROM messages
        WHERE session_id = new.session_id AND id <> new.id
          AND (active = 1 OR compacted = 1)
          AND display_identity = new.display_identity AND display_order IS NOT NULL
        ORDER BY display_order LIMIT 1
    ), new.id)) WHERE id = new.id
      AND (new.active = 1 OR new.compacted = 1);
    UPDATE messages SET display_order = (SELECT display_order FROM messages WHERE id = new.id)
    WHERE session_id = new.session_id AND id <> new.id AND (active = 1 OR compacted = 1)
      AND display_identity = new.display_identity
      AND (new.active = 1 OR new.compacted = 1);
    UPDATE messages SET display_order = (
        SELECT MIN(peer.id) FROM messages AS peer
        WHERE peer.session_id = old.session_id AND (peer.active = 1 OR peer.compacted = 1)
          AND peer.display_identity = old.display_identity
    ) WHERE session_id = old.session_id AND (active = 1 OR compacted = 1)
      AND display_identity = old.display_identity
      AND NOT (new.active = 1 OR new.compacted = 1);
END;
DROP TRIGGER IF EXISTS messages_display_identity_update;
CREATE TRIGGER IF NOT EXISTS messages_display_identity_update
AFTER UPDATE OF role, content, timestamp, tool_call_id, tool_calls, tool_name,
                display_kind ON messages
WHEN new.role IS NOT old.role
  OR new.content IS NOT old.content
  OR new.timestamp IS NOT old.timestamp
  OR new.tool_call_id IS NOT old.tool_call_id
  OR new.tool_calls IS NOT old.tool_calls
  OR new.tool_name IS NOT old.tool_name
  OR new.display_kind IS NOT old.display_kind
BEGIN
    UPDATE messages SET display_identity = NULL, display_order = NULL
    WHERE id = new.id OR (
        session_id = old.session_id AND display_identity = old.display_identity
        AND (active = 1 OR compacted = 1)
    );
END;
DROP TRIGGER IF EXISTS messages_display_identity_delete;
CREATE TRIGGER IF NOT EXISTS messages_display_identity_delete
AFTER DELETE ON messages WHEN old.active = 1 OR old.compacted = 1
BEGIN
    UPDATE messages SET display_order = (
        SELECT MIN(peer.id) FROM messages AS peer
        WHERE peer.session_id = old.session_id AND (peer.active = 1 OR peer.compacted = 1)
          AND peer.display_identity = old.display_identity
    ) WHERE session_id = old.session_id AND (active = 1 OR compacted = 1)
      AND display_identity = old.display_identity;
END;
CREATE INDEX IF NOT EXISTS idx_messages_active_null
    ON messages(active) WHERE active IS NULL;
-- Logical-identity recovery (agent.transcript_repair._active_logical_message_row) runs inside the write lock.
CREATE INDEX IF NOT EXISTS idx_messages_session_uid
    ON messages(session_id, message_uid) WHERE active = 1;
CREATE INDEX IF NOT EXISTS idx_sessions_session_key
    ON sessions(session_key, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_gateway_peer
    ON sessions(source, user_id, chat_id, chat_type, thread_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_handoff_state
    ON sessions(handoff_state, started_at);
CREATE INDEX IF NOT EXISTS idx_sessions_system_prompt_hash
    ON sessions(system_prompt_hash);
CREATE INDEX IF NOT EXISTS idx_sessions_tool_names
    ON sessions(tool_names);
-- Recent-session browsing must never derive recency by scanning messages.
-- This expression is the durable, indexable approximation used to preselect
-- a small candidate set before compression-chain and preview hydration.
CREATE INDEX IF NOT EXISTS idx_sessions_effective_activity
    ON sessions(COALESCE(last_activity_at, started_at) DESC, started_at DESC);
-- Upstream hermes_state_schema.py:135 creates this UNIQUE partial index on sessions(title).
CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_title_unique
    ON sessions(title) WHERE title IS NOT NULL;
-- Upstream hermes_state_schema.py:959 creates this partial index on messages(session_id, platform_message_id).
CREATE INDEX IF NOT EXISTS idx_messages_platform_msg_id
    ON messages(session_id, platform_message_id) WHERE platform_message_id IS NOT NULL;
"#;

/// (table, column, declared default or None) for additive reconciliation.
/// Covers the slice's write surfaces; upstream reconciles every table the
/// same way (additive, upstream's own defaults).
pub const RECONCILE_COLUMNS: &[(&str, &str, Option<&str>)] = &[
    ("sessions", "id", None),
    ("sessions", "source", None),
    ("sessions", "created_source", None),
    ("sessions", "user_id", None),
    ("sessions", "session_key", None),
    ("sessions", "chat_id", None),
    ("sessions", "chat_type", None),
    ("sessions", "thread_id", None),
    ("sessions", "display_name", None),
    ("sessions", "origin_json", None),
    ("sessions", "expiry_finalized", Some("0")),
    ("sessions", "model", None),
    ("sessions", "model_config", None),
    ("sessions", "system_prompt", None),
    ("sessions", "system_prompt_hash", None),
    ("sessions", "parent_session_id", None),
    ("sessions", "started_at", None),
    ("sessions", "ended_at", None),
    ("sessions", "end_reason", None),
    ("sessions", "message_count", Some("0")),
    ("sessions", "tool_call_count", Some("0")),
    ("sessions", "input_tokens", Some("0")),
    ("sessions", "output_tokens", Some("0")),
    ("sessions", "cache_read_tokens", Some("0")),
    ("sessions", "cache_write_tokens", Some("0")),
    ("sessions", "reasoning_tokens", Some("0")),
    ("sessions", "cwd", None),
    ("sessions", "git_branch", None),
    ("sessions", "git_repo_root", None),
    ("sessions", "git_metadata_generation", Some("0")),
    ("sessions", "billing_provider", None),
    ("sessions", "billing_base_url", None),
    ("sessions", "billing_mode", None),
    ("sessions", "estimated_cost_usd", None),
    ("sessions", "actual_cost_usd", None),
    ("sessions", "cost_status", None),
    ("sessions", "cost_source", None),
    ("sessions", "pricing_version", None),
    ("sessions", "title", None),
    ("sessions", "title_source", None),
    ("sessions", "last_activity_at", None),
    ("sessions", "last_activity_description", None),
    ("sessions", "last_activity_provenance", None),
    ("sessions", "api_call_count", Some("0")),
    ("sessions", "handoff_state", None),
    ("sessions", "handoff_platform", None),
    ("sessions", "handoff_error", None),
    ("sessions", "compression_failure_cooldown_until", None),
    ("sessions", "compression_failure_error", None),
    ("sessions", "compression_fallback_streak", Some("0")),
    ("sessions", "compression_ineffective_count", Some("0")),
    ("sessions", "compression_recovery_deadline", None),
    ("sessions", "compression_overload_streak", Some("0")),
    ("sessions", "profile_name", None),
    ("sessions", "transport_profile", None),
    ("sessions", "rewind_count", Some("0")),
    ("sessions", "archived", Some("0")),
    ("sessions", "auto_archived", Some("0")),
    ("sessions", "pinned", Some("0")),
    ("sessions", "hidden", Some("0")),
    ("sessions", "last_read_at", None),
    ("sessions", "tool_names", None),
    ("messages", "id", None),
    ("messages", "session_id", None),
    ("messages", "role", None),
    ("messages", "content", None),
    ("messages", "tool_call_id", None),
    ("messages", "tool_calls", None),
    ("messages", "tool_name", None),
    ("messages", "effect_disposition", None),
    ("messages", "timestamp", None),
    ("messages", "token_count", None),
    ("messages", "finish_reason", None),
    ("messages", "reasoning", None),
    ("messages", "reasoning_content", None),
    ("messages", "reasoning_details", None),
    ("messages", "codex_reasoning_items", None),
    ("messages", "codex_message_items", None),
    ("messages", "platform_message_id", None),
    ("messages", "observed", Some("0")),
    ("messages", "_compressed_summary", Some("0")),
    ("messages", "active", Some("1")),
    ("messages", "compacted", Some("0")),
    ("messages", "api_content", None),
    ("messages", "display_kind", None),
    ("messages", "display_metadata", None),
    ("messages", "display_identity", None),
    ("messages", "display_order", None),
    ("messages", "message_uid", None),
    ("messages", "absorbed_message_uids", None),
    ("messages", "tool_call_uids", None),
    ("messages", "tool_call_uid", None),
    ("session_model_usage", "session_id", None),
    ("session_model_usage", "model", None),
    ("session_model_usage", "billing_provider", Some("''")),
    ("session_model_usage", "billing_base_url", Some("''")),
    ("session_model_usage", "billing_mode", Some("''")),
    ("session_model_usage", "task", Some("''")),
    ("session_model_usage", "api_call_count", Some("0")),
    ("session_model_usage", "input_tokens", Some("0")),
    ("session_model_usage", "output_tokens", Some("0")),
    ("session_model_usage", "cache_read_tokens", Some("0")),
    ("session_model_usage", "cache_write_tokens", Some("0")),
    ("session_model_usage", "reasoning_tokens", Some("0")),
    ("session_model_usage", "estimated_cost_usd", Some("0")),
    ("session_model_usage", "actual_cost_usd", Some("0")),
    ("session_model_usage", "cost_status", None),
    ("session_model_usage", "cost_source", None),
    ("session_model_usage", "first_seen", None),
    ("session_model_usage", "last_seen", None),
];
pub const FRESH_STORE_CENSUS_JSON: &str = r#"{
 "index": [
  "idx_async_delegations_delivery",
  "idx_compression_locks_expires",
  "idx_messages_active_null",
  "idx_messages_assistant_calls_by_session",
  "idx_messages_display_backfill",
  "idx_messages_display_identity",
  "idx_messages_display_page",
  "idx_messages_platform_msg_id",
  "idx_messages_session",
  "idx_messages_session_active",
  "idx_messages_session_id",
  "idx_messages_session_uid",
  "idx_session_model_usage_model",
  "idx_session_model_usage_session",
  "idx_session_turn_leases_expires",
  "idx_sessions_effective_activity",
  "idx_sessions_gateway_peer",
  "idx_sessions_handoff_state",
  "idx_sessions_parent",
  "idx_sessions_session_key",
  "idx_sessions_source",
  "idx_sessions_source_id",
  "idx_sessions_started",
  "idx_sessions_system_prompt_hash",
  "idx_sessions_title_unique",
  "idx_sessions_tool_names"
 ],
 "table": [
  "async_delegations",
  "compression_locks",
  "conversation_generations",
  "gateway_heartbeats",
  "gateway_hygiene_state",
  "gateway_routing",
  "messages",
  "messages_fts",
  "messages_fts_config",
  "messages_fts_data",
  "messages_fts_docsize",
  "messages_fts_idx",
  "messages_fts_trigram",
  "messages_fts_trigram_config",
  "messages_fts_trigram_data",
  "messages_fts_trigram_docsize",
  "messages_fts_trigram_idx",
  "schema_version",
  "session_model_usage",
  "session_turn_leases",
  "sessions",
  "state_meta",
  "system_prompts"
 ],
 "trigger": [
  "messages_display_identity_delete",
  "messages_display_identity_update",
  "messages_display_order_insert",
  "messages_display_visibility_update",
  "messages_fts_delete",
  "messages_fts_insert",
  "messages_fts_trigram_delete",
  "messages_fts_trigram_insert",
  "messages_fts_trigram_update",
  "messages_fts_update",
  "messages_message_uid_insert"
 ],
 "view": [
  "messages_fts_src",
  "messages_fts_trigram_src"
 ]
}"#;
