#!/usr/bin/env python3
"""HRS-010 compatibility fixture generator.

Produces tests/fixtures/hrs010/state.db using the PINNED upstream Hermes
Agent v0.22.0 reference (specs/UPSTREAM_BASELINE.md) via its real
``SessionDB`` — the fixture is Python-produced in the strict sense: every
row, index, trigger, view and FTS index is created by the pinned
reference's own code paths, not by hand-written DDL.

The generator also emits manifest.json: the ground truth (exact session /
message / usage rows, sqlite_master census, FTS row counts, schema
version, producer provenance, sha256 of the produced file) that the Rust
tests and the sqlite3 check script assert against.

Run (from the repo root):

    /tmp/hermes-upstream-pin-hrs001/.venv/bin/python \
        scripts/make_hrs010_fixture.py

The pinned checkout path may be overridden with HERMES_RS_UPSTREAM_PIN.
The generator never touches a production store: the db_path is a scratch
path under this repo's tests/fixtures tree and the process is not a
pytest context (upstream's live-system guard does not fire; the explicit
db_path keeps us out of the default HERMES_HOME resolution anyway).
"""

from __future__ import annotations

import hashlib
import json
import os
import platform
import sqlite3
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
PIN = Path(os.environ.get("HERMES_RS_UPSTREAM_PIN", "/tmp/hermes-upstream-pin-hrs001"))
FIXTURE_DIR = REPO / "tests" / "fixtures" / "hrs010"
DB_PATH = FIXTURE_DIR / "state.db"

sys.path.insert(0, str(PIN))

# Fixed identifiers/uids/timestamps: the fixture must be reproducible in
# CONTENT across regenerations (ids minted by the caller in upstream
# shapes; message_uids pinned so the manifest's row identities are
# stable). Random store provenance (store_instance_id) is recorded
# post-hoc in the manifest.
SESSIONS = [
    {
        "id": "20261015_090000_a1b2c3",
        "source": "cli",
        "model": "glm-5.3-flash",
        "messages": [
            {
                "role": "user",
                "content": "What files live in src/persistence?",
                "timestamp": 1790000001.0,
                "message_uid": "f" + "0" * 31,
                "token_count": 12,
            },
            {
                "role": "assistant",
                "content": "The persistence module ships a stock SQLite backend.",
                "timestamp": 1790000002.5,
                "message_uid": "e" + "1" * 31,
                "token_count": 34,
            },
            {
                "role": "tool",
                "content": "mod.rs\nstock_sqlite.rs\nsession_store.rs",
                "tool_name": "list_files",
                "tool_call_id": "call_001",
                "tool_calls": None,
                "timestamp": 1790000003.0,
                "message_uid": "d" + "2" * 31,
                "token_count": 7,
            },
        ],
        "end_reason": None,
        "usage": {
            "input_tokens": 120,
            "output_tokens": 340,
            "cache_read_tokens": 50,
            "cache_write_tokens": 10,
            "reasoning_tokens": 20,
            "estimated_cost_usd": 0.0012,
            "api_call_count": 2,
            "billing_provider": "custom",
            "billing_base_url": "http://127.0.0.1:20128/v1",
            "billing_mode": "gateway",
        },
        "usage_model": "glm-5.3-flash",
    },
    {
        "id": "20261015_090500_b3c4d5",
        "source": "cli",
        "model": "glm-5.3-flash",
        "messages": [
            {
                "role": "user",
                "content": "Résumé the FTS5 rebuild rules for me",
                "timestamp": 1790001001.0,
                "message_uid": "c" + "3" * 31,
                "token_count": 9,
            },
            {
                "role": "assistant",
                "content": "Deferred rebuilds gate trigger membership on "
                "fts_rebuild_high_water / fts_rebuild_progress markers; "
                "unique_token_xyz is findable.",
                "timestamp": 1790001002.0,
                "message_uid": "b" + "4" * 31,
                "token_count": 41,
            },
        ],
        # Reset-class boundary: must bump conversation_generations for
        # (source, session_key) when the row actually changed.
        "end_reason": "idle",
        "session_key": "tg:1003310984808:2",
        "chat_id": "1003310984808",
        "chat_type": "topic",
        "user_id": "kara-test",
        "usage": None,
    },
    {
        "id": "20261015_091000_c4d5e6",
        "source": "gateway",
        "model": None,
        "messages": [
            {
                "role": "user",
                "content": "compression-continuation probe",
                "timestamp": 1790002001.0,
                "message_uid": "a" + "5" * 31,
                "token_count": 5,
            },
        ],
        # Non-reset boundary: ends the session but must NOT bump the
        # conversation generation (compression continues one conversation).
        "end_reason": "compression",
        "session_key": "tg:1003310984808:2",
        "chat_id": "1003310984808",
        "chat_type": "topic",
        "user_id": "kara-test",
        "parent_session_id": "20261015_090500_b3c4d5",
        "usage": None,
    },
]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def census(conn: sqlite3.Connection) -> dict[str, list[str]]:
    out: dict[str, list[str]] = {}
    for typ in ("table", "index", "view", "trigger"):
        rows = conn.execute(
            "SELECT name FROM sqlite_master WHERE type = ? AND name NOT LIKE 'sqlite_%' ORDER BY name",
            (typ,),
        ).fetchall()
        out[typ] = [r[0] for r in rows]
    return out


def main() -> int:
    from hermes_state import SessionDB  # pinned upstream reference

    FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
    for leftover in (DB_PATH, Path(str(DB_PATH) + "-wal"), Path(str(DB_PATH) + "-shm")):
        leftover.unlink(missing_ok=True)

    db = SessionDB(DB_PATH)
    try:
        for s in SESSIONS:
            kwargs: dict = {"model": s["model"]}
            for col in (
                "session_key",
                "chat_id",
                "chat_type",
                "user_id",
                "parent_session_id",
            ):
                if s.get(col):
                    kwargs[col] = s[col]
            db.create_session(s["id"], s["source"], **kwargs)
            for m in s["messages"]:
                db.append_message(
                    s["id"],
                    m["role"],
                    content=m["content"],
                    tool_name=m.get("tool_name"),
                    tool_calls=m.get("tool_calls"),
                    tool_call_id=m.get("tool_call_id"),
                    token_count=m.get("token_count"),
                    timestamp=m["timestamp"],
                    message_uid=m["message_uid"],
                )
            u = s.get("usage")
            if u:
                db.update_token_counts(
                    s["id"], source=s["source"], model=s.get("usage_model"), **u
                )
            if s["end_reason"]:
                db.end_session(s["id"], s["end_reason"])
        db.flush_token_counts()

        # Ground-truth reads back through the reference itself.
        truth_sessions = {}
        for s in SESSIONS:
            row = db.get_session(s["id"])
            msgs = db.get_messages(s["id"])
            truth_sessions[s["id"]] = {
                "row": {
                    k: row[k]
                    for k in (
                        "id",
                        "source",
                        "model",
                        "message_count",
                        "tool_call_count",
                        "input_tokens",
                        "output_tokens",
                        "cache_read_tokens",
                        "cache_write_tokens",
                        "reasoning_tokens",
                        "started_at",
                        "ended_at",
                        "end_reason",
                        "billing_provider",
                        "billing_base_url",
                        "billing_mode",
                        "api_call_count",
                        "estimated_cost_usd",
                        "parent_session_id",
                        "session_key",
                        "chat_id",
                        "chat_type",
                        "user_id",
                    )
                },
                "messages": [
                    {
                        k: mm[k]
                        for k in (
                            "id",
                            "role",
                            "content",
                            "timestamp",
                            "message_uid",
                            "token_count",
                            "tool_call_id",
                            "tool_name",
                            "active",
                            "compacted",
                        )
                    }
                    for mm in msgs
                ],
            }
        hits = db.search_messages("unique_token_xyz")
        truth_search = [(h["id"], h["session_id"]) for h in hits]
        assert truth_search, (
            "upstream search must find the unique token in the fresh fixture"
        )
    finally:
        db.close()

    # Clean close must have unlinked WAL sidecars; verify and fall back to an
    # explicit truncate-checkpoint pass if not.
    wal = Path(str(DB_PATH) + "-wal")
    if wal.exists():
        ck = sqlite3.connect(DB_PATH)
        ck.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        ck.close()
    assert not wal.exists(), "fixture must be a single self-contained state.db"

    # Upstream runtime leaves admission lock sidecars (<db>.repair.lock /
    # <db>.fts_rebuild.lock / quarantine lock); they are transient host
    # coordination artifacts, NOT part of the compatibility surface. Remove
    # them so the committed fixture directory is exactly {state.db, manifest}.
    for suffix in (".repair.lock", ".fts_rebuild.lock", ".quarantine.lock"):
        Path(str(DB_PATH) + suffix).unlink(missing_ok=True)

    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    schema_version = conn.execute(
        "SELECT version FROM schema_version LIMIT 1"
    ).fetchone()[0]
    meta = {
        r["key"]: r["value"] for r in conn.execute("SELECT key, value FROM state_meta")
    }
    fts_counts = {
        "messages": conn.execute("SELECT COUNT(*) FROM messages").fetchone()[0],
        "messages_fts": conn.execute("SELECT COUNT(*) FROM messages_fts").fetchone()[0],
    }
    try:
        fts_counts["messages_fts_trigram"] = conn.execute(
            "SELECT COUNT(*) FROM messages_fts_trigram"
        ).fetchone()[0]
    except sqlite3.OperationalError as exc:
        fts_counts["messages_fts_trigram"] = f"unavailable: {exc}"
    try:
        fts_counts["messages_fts_cjk"] = conn.execute(
            "SELECT COUNT(*) FROM messages_fts_cjk"
        ).fetchone()[0]
    except sqlite3.OperationalError as exc:
        fts_counts["messages_fts_cjk"] = f"unavailable: {exc}"
    generations = [
        dict(r)
        for r in conn.execute(
            "SELECT source, session_key, generation FROM conversation_generations ORDER BY source, session_key"
        )
    ]
    usage_rows = [
        dict(r)
        for r in conn.execute(
            "SELECT * FROM session_model_usage ORDER BY session_id, model, task"
        )
    ]
    journal_mode = conn.execute("PRAGMA journal_mode").fetchone()[0]
    integrity = conn.execute("PRAGMA integrity_check").fetchone()[0]
    master = census(conn)
    conn.close()

    assert integrity == "ok", integrity
    manifest = {
        "fixture": "tests/fixtures/hrs010/state.db",
        "producer": {
            "reference": "Hermes Agent (pinned upstream, specs/UPSTREAM_BASELINE.md)",
            "pin": "v0.22.0 @ e06df9c2eae6901f4c88f464e0ae6e753c0428b8",
            "checkout": str(PIN),
            "python": sys.version.split()[0],
            "sqlite_lib": sqlite3.sqlite_version,
            "platform": platform.platform(),
            "generator": "scripts/make_hrs010_fixture.py",
        },
        "schema_version": schema_version,
        "state_meta": meta,
        "journal_mode_at_rest": journal_mode,
        "fts_counts": fts_counts,
        "conversation_generations": generations,
        "session_model_usage": usage_rows,
        "sqlite_master_census": master,
        "search": {"unique_token_xyz": truth_search},
        "sessions": truth_sessions,
        "state_db_sha256": sha256(DB_PATH),
    }
    (FIXTURE_DIR / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    print(f"fixture: {DB_PATH} sha256={manifest['state_db_sha256']}")
    print(
        f"schema_version={schema_version} messages={fts_counts['messages']} "
        f"fts={fts_counts['messages_fts']} trigram={fts_counts['messages_fts_trigram']} "
        f"cjk={fts_counts['messages_fts_cjk']}"
    )
    print(f"generations={generations}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
