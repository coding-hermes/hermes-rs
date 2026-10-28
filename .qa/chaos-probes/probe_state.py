#!/usr/bin/env python3
"""DF-HRS-120 corruption arms for the runtime SQLite state store.

Drives the DOCUMENTED Python client (scripts/hrs003_client.py) to create a real
runtime state.db, proves the clean control with stdlib sqlite3
(PRAGMA integrity_check == "ok"), then deterministically byte-flips a copy
until PRAGMA integrity_check fails with "database disk image is malformed".

Called by .qa/chaos-probes/corruption.sh. Exit 0 = OK, 1 = FAIL (product
misbehavior), 2 = INFO (environment gap). The FIRST stdout line is the cell
detail consumed by the QA battery.
"""

from __future__ import annotations

import os
import shutil
import sqlite3
import sys

SQLITE_VERSION_STR = "sqlite3 " + sqlite3.sqlite_version + " (python stdlib)"
CORRUPT_SIGNAL = "database disk image is malformed"
# Brief canonical bound is 8; widened to 17 (measured, same ascending ladder):
# on a fresh runtime store, pages 2-9 are near-empty schema-root b-trees whose
# cell-pointer arrays end before offset 16, so flips at k=1..8 land in unused
# space and integrity_check keeps reporting ok. The first populated leaves
# (sqlite_autoindex on schema objects, then data-table leaves) start around
# k=10-11, where a +16 flip deterministically breaks the cell-pointer array.
MAX_FLIPS = 17
# Additional append_message calls so the runtime store carries real message
# content (same documented client method; the messages leaf then participates
# in the corruption surface).
POPULATE_MESSAGES = 40


def fail(msg: str, detail: list[str] | None = None) -> int:
    print(msg)
    for line in detail or []:
        print("  " + line)
    return 1


def info(msg: str) -> int:
    print(msg)
    return 2


def integrity_attempt(db: str) -> tuple[str, str]:
    """Run PRAGMA integrity_check to exhaustion; answer (status, text).

    status is "ok", "malformed" (the literal corruption signal, found in any
    result row or raised as a sqlite error), or "other". integrity_check
    returns one row per problem plus a final "ok" row; the corruption signal
    may not be the first row (e.g. a schema-object error can precede the
    b-tree corruption report), so the signal is searched across all rows.
    """
    con = sqlite3.connect(db)
    try:
        rows = [r[0] for r in con.execute("PRAGMA integrity_check").fetchall()]
    except sqlite3.DatabaseError as e:
        if CORRUPT_SIGNAL in str(e):
            return "malformed", CORRUPT_SIGNAL
        return "other", str(e)
    finally:
        con.close()
    if any(CORRUPT_SIGNAL in row for row in rows):
        return "malformed", CORRUPT_SIGNAL
    if rows == ["ok"]:
        return "ok", "ok"
    return "other", "; ".join(rows)


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        return info(
            "CORRUPTION_PROBE_INFO usage: probe_state.py ADAPTER SCRATCH REPO_ROOT"
        )
    adapter, scratch, repo_root = argv[1], argv[2], argv[3]

    sys.path.insert(0, os.path.join(repo_root, "scripts"))
    try:
        from hrs003_client import SessionAdapter  # documented client, stdlib only
    except ImportError as e:
        return info(
            "CORRUPTION_PROBE_INFO cannot import scripts/hrs003_client.py: %s" % e
        )

    db = os.path.join(scratch, "state.db")
    try:
        client = SessionAdapter(adapter)
        try:
            client.open(db, create=True)
            client.ensure_session("20261015_120000_aa0001", source="cli", model=None)
            client.append_message(
                "20261015_120000_aa0001", "user", "corruption-probe message"
            )
            for i in range(1, POPULATE_MESSAGES):
                client.append_message(
                    "20261015_120000_aa0001",
                    "user",
                    "corruption-probe message %03d" % i,
                )
            client.checkpoint()
        finally:
            client.shutdown()
    except Exception as e:  # adapter/protocol failures are product-observable
        return fail(
            "FAIL_CLIENT_RUNTIME creating runtime state.db through hrs003_client failed: %r"
            % (e,),
        )
    if not os.path.exists(db):
        return fail("FAIL_NO_DB runtime state.db absent after client shutdown: %s" % db)

    # 5. Clean control: integrity_check on the real runtime db must report ok.
    status, text = integrity_attempt(db)
    if status != "ok":
        return fail(
            "FAIL_CLEAN_CONTROL clean integrity_check=%r (expected 'ok')" % text,
            detail=["sqlite: " + SQLITE_VERSION_STR],
        )

    # 6. Corruption arm: deterministic single-byte flips on a copy, bounded,
    #    ascending; a flip whose copy stays healthy is restored before the next.
    corrupt_db = os.path.join(scratch, "state-corrupt.db")
    offsets_tried = 0
    corrupted_offset = None
    signal_result = None
    last_result = "no candidate tried"
    try:
        for k in range(1, MAX_FLIPS + 1):
            offset = 4096 * k + 16
            shutil.copyfile(db, corrupt_db)
            try:
                with open(corrupt_db, "r+b") as fh:
                    fh.seek(offset)
                    orig = fh.read(1)
                    fh.seek(offset)
                    fh.write(bytes([orig[0] ^ 0xFF]))
            except OSError as e:
                return fail(
                    "FAIL_FLIP_IO byte flip at offset %d failed: %s" % (offset, e),
                    detail=["sqlite: " + SQLITE_VERSION_STR],
                )
            offsets_tried += 1
            status, text = integrity_attempt(corrupt_db)
            if status == "malformed":
                corrupted_offset = offset
                signal_result = text
                break
            # Copy still healthy -> restore the pristine bytes before the next
            # candidate so flips never accumulate.
            shutil.copyfile(db, corrupt_db)
            last_result = text
    finally:
        if os.path.exists(corrupt_db):
            os.remove(corrupt_db)

    if corrupted_offset is None:
        return fail(
            "FAIL_NO_CORRUPTION no offset corrupted the db within %d flips; last integrity_check=%r"
            % (MAX_FLIPS, last_result),
            detail=["sqlite: " + SQLITE_VERSION_STR],
        )

    print(
        "CORRUPTION_PROBE_OK clean=ok corrupt_signal=%r offsets_tried=%d"
        % (signal_result, offsets_tried)
    )
    print("  sqlite: " + SQLITE_VERSION_STR)
    print(
        "  clean control: PRAGMA integrity_check on the client-created runtime state.db = ok"
    )
    print(
        "  corrupt arm: byte-flip at offset %d (k=%d) -> integrity_check %r (sqlite error, nonzero)"
        % (corrupted_offset, corrupted_offset // 4096 - 1, signal_result)
    )
    print(
        "  client path: SessionAdapter(adapter).open(db, create=True) -> ensure_session -> append_message x%d -> checkpoint -> shutdown"
        % POPULATE_MESSAGES
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
