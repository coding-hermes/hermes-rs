#!/usr/bin/env python3
"""Generate src/persistence/upstream_ddl.rs from the pinned upstream checkout.

Embeds the exact SCHEMA_SQL, FTS_SQL, FTS_TRIGRAM_SQL, DEFERRED_INDEX_SQL
string values of the pinned reference (v0.22.0 @ e06df9c2) plus a
(table, column, default) reconciliation manifest for the slice's tables,
read from a freshly-scaffolded upstream store via PRAGMA table_info.
Regenerate only on a re-pin.
"""

from __future__ import annotations

import os
import sqlite3
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
PIN = Path(os.environ.get("HERMES_RS_UPSTREAM_PIN", "/tmp/hermes-upstream-pin-hrs001"))
OUT = REPO / "src" / "persistence" / "upstream_ddl.rs"

sys.path.insert(0, str(PIN))

RECONCILE_TABLES = ("sessions", "messages", "session_model_usage")


def rust_raw(name: str, sql: str) -> str:
    # Choose a hash count that cannot appear in the SQL text.
    hashes = "#"
    while f'"{hashes}' in sql or f'{hashes}"' in sql:
        hashes += "#"
    return f'pub const {name}: &str = r{hashes}"{sql}"{hashes};\n\n'


def main() -> int:
    import hermes_state_common as common

    chunks: list[str] = [
        "//! DDL + column manifest extracted VERBATIM from the pinned upstream\n"
        "//! checkout (v0.22.0 @ e06df9c2eae6901f4c88f464e0ae6e753c0428b8) by\n"
        "//! scripts/gen_upstream_ddl.py. Do not hand-edit; regenerate on re-pin.\n\n"
    ]
    for name in ("SCHEMA_SQL", "FTS_SQL", "FTS_TRIGRAM_SQL", "DEFERRED_INDEX_SQL"):
        sql = getattr(common, name)
        chunks.append(rust_raw(f"UPSTREAM_{name}", sql))

    # Column manifest from a fresh upstream store.
    from hermes_state import SessionDB

    with tempfile.TemporaryDirectory() as td:
        dbp = Path(td) / "state.db"
        db = SessionDB(dbp)
        db.close()
        wal = Path(str(dbp) + "-wal")
        if wal.exists():
            ck = sqlite3.connect(dbp)
            ck.execute("PRAGMA wal_checkpoint(TRUNCATE)")
            ck.close()
        conn = sqlite3.connect(dbp)
        conn.row_factory = sqlite3.Row
        lines = [
            "/// (table, column, declared default or None) for additive reconciliation.",
            "/// Covers the slice's write surfaces; upstream reconciles every table the",
            "/// same way (additive, upstream's own defaults).",
            "pub const RECONCILE_COLUMNS: &[(&str, &str, Option<&str>)] = &[",
        ]
        for table in RECONCILE_TABLES:
            for col in conn.execute(f"PRAGMA table_info({table})"):
                dflt = col["dflt_value"]
                if dflt is None:
                    token = "None"
                else:
                    token = 'Some("%s")' % str(dflt).replace("\\", "\\\\").replace(
                        '"', '\\"'
                    )
                lines.append('    ("%s", "%s", %s),' % (table, col["name"], token))
        lines.append("];\n")
        chunks.append("\n".join(lines))

        # Trigger / view / FTS object census of a FRESH upstream store, for the
        # fresh-store F-10 leg.
        fresh_census: dict[str, list[str]] = {}
        for typ in ("table", "index", "view", "trigger"):
            rows = conn.execute(
                "SELECT name FROM sqlite_master WHERE type = ? AND name NOT LIKE 'sqlite_%' ORDER BY name",
                (typ,),
            ).fetchall()
            fresh_census[typ] = [r[0] for r in rows]
        conn.close()
    chunks.append(
        rust_raw(
            "FRESH_STORE_CENSUS_JSON",
            __import__("json").dumps(fresh_census, sort_keys=True, indent=1),
        )
    )
    OUT.write_text("".join(chunks), encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
