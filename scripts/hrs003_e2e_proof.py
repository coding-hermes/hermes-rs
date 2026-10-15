#!/usr/bin/env python3
"""HRS-003 end-to-end interop proof driver.

Mode "proof" (default):
  1. launches the Rust session-store adapter as a subprocess from real
     CPython via the stdlib-only client (scripts/hrs003_client.py);
  2. proves structured round-trip: create / append / read / REOPEN the
     same SQLite file and read again through a SECOND adapter process;
  3. proves the independent read-back: the stdlib `sqlite3` module opens
     the same file directly (a different SQLite stack than the one the
     Rust build links) and sees the acknowledged rows;
  4. proves structured error propagation: appending to a nonexistent
     session returns a structured AdapterError, not a crash;
  5. proves lifecycle: clean shutdown, exit 0, no orphaned adapter
     process and no leftover lock artifacts after the final close.

Mode "broken":
  Deliberately drives a broken call sequence (append to a nonexistent
  session) and captures the failing output into the requested file. This
  is the non-vacuity evidence required by the task contract — the proof
  tests require this transcript to exist and to name the error class.

Exit codes: 0 = the mode's contract held; 1 = it did not.
"""

from __future__ import annotations

import argparse
import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from hrs003_client import AdapterError, SessionAdapter  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ADAPTER_DEFAULT = os.path.join(REPO, "target", "debug", "hermes-rs-session-adapter")


def resolve_adapter(cli_value: str | None) -> str:
    """Adapter binary path: --adapter wins, then the shared cargo target
    dir (CARGO_TARGET_DIR convention), then the repo-local default. A
    missing binary is built (standalone/manual runs); cargo tests always
    pass an explicit, task-unique copy."""
    if cli_value:
        return cli_value
    env_dir = os.environ.get("CARGO_TARGET_DIR")
    if env_dir and os.path.exists(
        os.path.join(env_dir, "debug", "hermes-rs-session-adapter")
    ):
        return os.path.join(env_dir, "debug", "hermes-rs-session-adapter")
    return ADAPTER_DEFAULT


def build_adapter(adapter: str) -> None:
    """Ensure the adapter binary exists (cargo build is idempotent)."""
    if os.path.exists(adapter):
        return
    subprocess.run(
        ["cargo", "build", "--bin", "hermes-rs-session-adapter"],
        cwd=REPO,
        check=True,
        capture_output=True,
    )


def roundtrip_proof(scratch: str, adapter: str) -> None:
    build_adapter(adapter)
    db = os.path.join(scratch, "state.db")
    uid_a = f"{int(time.time())}a"  # content marker, message A
    uid_b = f"{int(time.time())}b"  # content marker, message B

    # ── process 1: create + append + end ────────────────────────────────
    with SessionAdapter(adapter) as p1:
        report = p1.open(db, create=True)
        assert report.schema_version_after == 31, report
        p1.ensure_session(
            "20261015_120000_aa0001", source="hrs003-proof", model="proof-model"
        )
        r1 = p1.append_message(
            "20261015_120000_aa0001", "user", f"hello from Python ({uid_a})"
        )
        r2 = p1.append_message(
            "20261015_120000_aa0001",
            "assistant",
            f"reply from the Rust store ({uid_b})",
            tool_calls=json.dumps([{"id": "call_1", "function": {"name": "ping"}}]),
            tool_call_id="call_1",
            tool_name="ping",
        )
        assert r1["message_id"] > 0 and r2["message_id"] > r1["message_id"], (r1, r2)
        got = p1.get_messages("20261015_120000_aa0001")
        contents = [m["content"] for m in got["messages"]]
        assert any(uid_a in c for c in contents) and any(
            uid_b in c for c in contents
        ), contents
        end = p1.end_session("20261015_120000_aa0001", "session_reset")
        assert end["status"] == "ended", end
        p1.checkpoint()
        p1.backup(os.path.join(scratch, "state-backup.db"))
        p1.close_store()
    # SessionAdapter.__exit__ sent shutdown; prove the process is gone.

    # ── process 2: REOPEN the same file through a fresh adapter ─────────
    with SessionAdapter(adapter) as p2:
        report2 = p2.open(db)
        assert report2.schema_version_after == 31
        got2 = p2.get_messages("20261015_120000_aa0001")
        contents2 = [m["content"] for m in got2["messages"]]
        assert any(uid_a in c for c in contents2) and any(
            uid_b in c for c in contents2
        ), contents2
        assert got2["messages"][0]["role"] == "user"
        # session counters survived the round-trip
        session_row = p2._call("get_session", {"session_id": "20261015_120000_aa0001"})
        assert session_row["message_count"] == 2, session_row
        assert session_row["end_reason"] == "session_reset", session_row
        p2.close_store()

    print(
        "ROUNDTRIP_OK create/append/read/reopen verified across two adapter processes"
    )

    # ── independent read-back with stdlib sqlite3 (different SQLite stack) ──
    con = sqlite3.connect(db)
    try:
        cur = con.execute(
            "SELECT m.role, m.content FROM messages m WHERE m.session_id = ? ORDER BY m.id",
            ("20261015_120000_aa0001",),
        )
        rows = cur.fetchall()
        assert len(rows) == 2, rows
        assert rows[0][0] == "user" and uid_a in rows[0][1], rows
        schema_v = con.execute("SELECT version FROM schema_version LIMIT 1").fetchone()[
            0
        ]
        integrity = con.execute("PRAGMA integrity_check").fetchone()[0]
        session = con.execute(
            "SELECT message_count, end_reason FROM sessions WHERE id = ?",
            ("20261015_120000_aa0001",),
        ).fetchone()
        assert session == (2, "session_reset"), session
        backup_ok = (
            sqlite3.connect(os.path.join(scratch, "state-backup.db"))
            .execute("SELECT COUNT(*) FROM messages")
            .fetchone()[0]
        )
        assert backup_ok == 2, backup_ok
    finally:
        con.close()
    print(
        f"SQLITE3_REOPEN_OK stdlib sqlite3 read the acknowledged rows; integrity={integrity}"
    )
    print(f"SQLITE_SCHEMA_VERSION={schema_v}")

    # ── structured error propagation (in the same live session) ─────────
    with SessionAdapter(adapter) as p3:
        p3.open(db)
        try:
            p3.append_message("no_such_session_zz", "user", "this must fail")
            raise AssertionError("append into a nonexistent session must fail")
        except AdapterError as e:
            assert e.error == "SessionRowMissing", e
            print(
                f"ERROR_STRUCTURED bad call -> AdapterError({e.error}, cause={e.cause})"
            )
        # the failed call left the store usable: a good call still works
        r = p3.append_message(
            "20261015_120000_aa0001", "user", "store still usable after error"
        )
        assert r["message_id"] > 2, r
        p3.close_store()

    # ── lifecycle: clean stop, no orphaned adapter, no lock leftovers ───
    # Self-match-safe check (pgrep -f would match THIS driver's own argv,
    # which carries --adapter <path>): flag only processes whose argv[0]
    # is exactly the adapter binary.
    def adapter_pids() -> list[int]:
        pids: list[int] = []
        mine = {os.getpid(), os.getppid()}
        for entry in os.listdir("/proc"):
            if not entry.isdigit() or int(entry) in mine:
                continue
            try:
                with open(f"/proc/{entry}/cmdline", "rb") as f:
                    argv = f.read().split(b"\0")
            except (FileNotFoundError, ProcessLookupError, PermissionError):
                continue
            if argv and argv[0].decode(errors="replace") == adapter:
                pids.append(int(entry))
        return pids

    leftover = adapter_pids()
    assert leftover == [], f"orphaned adapter processes: {leftover}"
    sidecars = sorted(
        f
        for f in os.listdir(scratch)
        if f.startswith("state.db") and (f.endswith("-wal") or f.endswith("-shm"))
    )
    # After the final clean close, SQLite unlinks WAL sidecars; nothing may
    # hold a lock artifact. (Backup files are named state-backup.db.)
    assert sidecars == [], f"leftover WAL sidecars after clean close: {sidecars}"
    print("LIFECYCLE_CLEAN exit-0 shutdown, no orphaned adapter, no WAL/lock leftovers")


def broken_proof(out_path: str, adapter: str) -> None:
    """Non-vacuity: the broken case FAILS, and we record exactly how."""
    build_adapter(adapter)
    lines: list[str] = []
    lines.append("# HRS-003 non-vacuity evidence (deliberately broken call)")
    lines.append(f"Date: {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    lines.append(
        f"Python: {sys.version.split()[0]}  sqlite3 lib: {sqlite3.sqlite_version}"
    )
    lines.append(f"Adapter binary: {adapter}")
    lines.append(
        "Mutation: append_message against a session id that was never ensured."
    )
    lines.append("")

    scratch = tempfile.mkdtemp(prefix="hrs003-broken-")
    db = os.path.join(scratch, "state.db")
    ok = False
    try:
        with SessionAdapter(adapter) as p:
            p.open(db, create=True)
            # The store scaffolds fine; the call below is the broken one:
            # no ensure_session was issued for this id.
            try:
                p.append_message("20261015_999999_zz9999", "user", "poison append")
                lines.append(
                    "UNEXPECTED: append into a nonexistent session SUCCEEDED — "
                    "the negative proof FAILED (test would be vacuous)."
                )
                lines.append("BROKEN_CASE_FAILED_TO_FAIL")
            except AdapterError as e:
                ok = e.error == "SessionRowMissing"
                lines.append(
                    f"observed: AdapterError(error={e.error!r}, cause={e.cause!r}, detail={e.detail!r})"
                )
                lines.append(
                    f"process remained alive and answered a follow-up ping: "
                    f"{p._call('ping')}"
                )
                lines.append(
                    "BROKEN_CASE_VERIFIED" if ok else "BROKEN_CASE_WRONG_ERROR_CLASS"
                )
            p.close_store()
    finally:
        os.makedirs(os.path.dirname(out_path), exist_ok=True)
        with open(out_path, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")

    if not ok:
        print(
            f"broken case did not produce the expected failure; transcript at {out_path}"
        )
        sys.exit(1)
    print(f"BROKEN_CASE_VERIFIED structured error captured to {out_path}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--mode", choices=["proof", "broken"], default="proof")
    ap.add_argument("--out", default=None, help="transcript path for --mode broken")
    ap.add_argument(
        "--adapter",
        default=None,
        help="adapter binary path (cargo tests pass a task-unique copy)",
    )
    args = ap.parse_args()
    adapter = resolve_adapter(args.adapter)

    if args.mode == "broken":
        if not args.out:
            print("--out is required for --mode broken", file=sys.stderr)
            return 1
        broken_proof(args.out, adapter)
        return 0

    scratch = tempfile.mkdtemp(prefix="hrs003-proof-")
    try:
        roundtrip_proof(scratch, adapter)
    finally:
        subprocess.run(["rm", "-rf", scratch], check=False)
    return 0


if __name__ == "__main__":
    sys.exit(main())
