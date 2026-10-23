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

Mode "batch-wire" (DF-HRS-112):
  Drives append_messages_batch and record_model_usage end-to-end over the
  stdio JSON-RPC boundary against the built adapter binary — the two arms
  that exist only in the adapter's wire dispatch. Builds the RELEASE
  adapter on standalone/CI runs; cargo tests pass --adapter (task-unique
  copy) and skip the build.

Mode "bulk-wire" (DF-HRS-112):
  Consumer-scale re-measurement of the 2026-10-17 bulk-import claim with
  the batch API on the wire: per-message append_message RPCs (arm A) vs
  one append_messages_batch RPC (arm B) vs the stdlib one-transaction
  control (arm C), repeated --bulk-runs times, median reported.

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


def resolve_release_adapter(cli_value: str | None) -> str:
    """Adapter binary for the batch/usage wire leg (DF-HRS-112).

    Standalone and CI runs build the RELEASE profile per the task contract
    (cargo build --release --bin hermes-rs-session-adapter) and return that
    path. Cargo-test runs pass --adapter (a task-unique debug copy, shared
    CARGO_TARGET_DIR clobber protection) and reuse it: the wire contract
    under test is profile-independent, and a nested cargo build inside a
    cargo test would race the shared target dir."""
    if cli_value:
        return cli_value
    subprocess.run(
        ["cargo", "build", "--release", "--bin", "hermes-rs-session-adapter"],
        cwd=REPO,
        check=True,
    )
    env_dir = os.environ.get("CARGO_TARGET_DIR")
    target_root = env_dir or os.path.join(REPO, "target")
    release = os.path.join(target_root, "release", "hermes-rs-session-adapter")
    if not os.path.exists(release):
        raise AssertionError(f"release adapter not found after build: {release}")
    return release


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
        # ── DF-HRS-112 wire arms: batch append + model usage ────────────
        batch_msgs = [
            {"role": "user", "content": f"batch msg {i} ({uid_a})"} for i in range(5)
        ]
        batch_r = p1.append_messages_batch("20261015_120000_aa0001", batch_msgs)
        assert batch_r["first_id"] > 0, batch_r
        assert batch_r["appended"] == 5, batch_r
        print(
            f"BATCH_WIRE_OK first_id={batch_r['first_id']} appended={batch_r['appended']}"
        )

        usage_r = p1.record_model_usage(
            "20261015_120000_aa0001",
            model="proof-model",
            input_tokens=100,
            output_tokens=50,
        )
        assert usage_r.get("ok") is True or "usage_id" in usage_r or usage_r == {}, (
            usage_r
        )
        print(f"MODEL_USAGE_WIRE_OK response={usage_r}")

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
        # session counters survived the round-trip. DF-HRS-119: the projection
        # is read through the PUBLIC client method (never the private `_call`)
        # and pinned to the message list it counts — a projection whose
        # message_count disagrees with len(get_messages) is a wire-contract
        # break, not a cosmetic one.
        session_row = p2.get_session("20261015_120000_aa0001")
        assert session_row is not None, "get_session returned null for a live session"
        assert session_row["message_count"] == 7, (
            session_row
        )  # 2 original + 5 batch (DF-HRS-112)
        assert session_row["end_reason"] == "session_reset", session_row
        assert session_row["message_count"] == len(got2["messages"]), (
            session_row["message_count"],
            len(got2["messages"]),
        )
        # A missing row is a null projection, not an error (§4).
        assert p2.get_session("no_such_session_zz") is None, (
            "get_session for an absent session must answer null/None"
        )
        print(
            f"GET_SESSION_WIRE_OK message_count={session_row['message_count']} "
            f"== len(get_messages)={len(got2['messages'])}"
        )
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
        assert len(rows) == 7, rows  # 2 original + 5 batch (DF-HRS-112)
        assert rows[0][0] == "user" and uid_a in rows[0][1], rows
        schema_v = con.execute("SELECT version FROM schema_version LIMIT 1").fetchone()[
            0
        ]
        integrity = con.execute("PRAGMA integrity_check").fetchone()[0]
        session = con.execute(
            "SELECT message_count, end_reason FROM sessions WHERE id = ?",
            ("20261015_120000_aa0001",),
        ).fetchone()
        assert session == (7, "session_reset"), (
            session
        )  # 2 original + 5 batch (DF-HRS-112)
        backup_ok = (
            sqlite3.connect(os.path.join(scratch, "state-backup.db"))
            .execute("SELECT COUNT(*) FROM messages")
            .fetchone()[0]
        )
        assert backup_ok == 7, backup_ok  # 2 original + 5 batch (DF-HRS-112)
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


def batch_usage_wire_proof(adapter: str, n: int = 200) -> None:
    """DF-HRS-112 wire leg: drive append_messages_batch and
    record_model_usage end-to-end over the stdio JSON-RPC boundary.

    These two arms exist only in src/bin/session_adapter.rs dispatch —
    no Rust-side test exercises the wire path for them, so a regression in
    the dispatch arms (param names, reply shape) would previously go
    unnoticed. Assertions here are the contract: first_id > 0,
    appended == N, counters survive a reopen, usage lands on the session
    row, and the reply is the documented {"ok": true}.
    """
    scratch = tempfile.mkdtemp(prefix="hrs112-wire-")
    db = os.path.join(scratch, "state.db")
    sid = "20261018_120000_bb0001"
    uid = f"hrs112-{int(time.time())}"
    with SessionAdapter(adapter) as p:
        report = p.open(db, create=True)
        assert report.schema_version_after == 31, report
        p.ensure_session(sid, source="hrs112-proof", model="proof-model")

        messages = [
            {"role": "user" if i % 2 == 0 else "assistant", "content": f"{uid} msg {i}"}
            for i in range(n)
        ]
        batch = p.append_messages_batch(sid, messages)
        assert batch["appended"] == n, batch
        assert batch["first_id"] > 0, batch

        got = p.get_messages(sid)
        assert len(got["messages"]) == n, len(got["messages"])
        assert uid in got["messages"][0]["content"], got["messages"][0]

        usage = p.record_model_usage(
            sid,
            model="glm-5.3-flash",
            billing_provider="zai",
            billing_base_url="https://api.z.ai",
            billing_mode="plan",
            task="hrs112-wire-proof",
            api_call_count=3,
            input_tokens=1000,
            output_tokens=200,
            cache_read_tokens=50,
            cache_write_tokens=60,
            reasoning_tokens=7,
            estimated_cost_usd=0.0123,
            cost_status="measured",
            cost_source="hrs112",
        )
        assert usage.get("ok") is True, usage

        row = p.get_session(sid)
        assert row is not None, "get_session returned null for a live session"
        assert row["message_count"] == n, row
        assert row["message_count"] == len(got["messages"]), (
            row["message_count"],
            len(got["messages"]),
        )
        assert row["input_tokens"] == 1000, row
        assert row["output_tokens"] == 200, row
        assert row["api_call_count"] == 3, row

        p.checkpoint()
        p.close_store()

    # Reopen through a second process: acknowledged batch rows and the
    # usage deltas are durable across the process boundary.
    with SessionAdapter(adapter) as p2:
        p2.open(db)
        got2 = p2.get_messages(sid)
        assert len(got2["messages"]) == n, len(got2["messages"])
        row2 = p2.get_session(sid)
        assert row2 is not None, "get_session returned null after reopen"
        assert row2["message_count"] == n and row2["input_tokens"] == 1000, row2
        assert row2["message_count"] == len(got2["messages"]), (
            row2["message_count"],
            len(got2["messages"]),
        )
        p2.close_store()

    print(
        f"GET_SESSION_BATCH_WIRE_OK message_count={row2['message_count']} "
        f"== len(get_messages)={len(got2['messages'])}"
    )

    print(
        f"BATCH_WIRE_OK append_messages_batch n={n} first_id={batch['first_id']} "
        f"appended={batch['appended']}"
    )
    print("MODEL_USAGE_WIRE_OK record_model_usage -> ok=true, counters verified")
    print("HRS112_WIRE_CLEAN batch + model usage verified across two processes")


def bulk_wire_measurement(
    adapter: str, n: int, runs: int, out_path: str, release_bin: str
) -> None:
    """DF-HRS-112 consumer-scale A/B: batch API over the WIRE vs the same
    N messages through per-message append_message calls over the wire, plus
    the stdlib one-transaction control from the original 2026-10-17
    measurement. Repeats the whole protocol `runs` times and reports every
    sample; no single-run numbers are quoted as conclusions."""
    scratch = tempfile.mkdtemp(prefix="hrs112-bulk-")
    body = [
        ("user", f"bulk message {i} about sqlite and wal checkpointing")
        for i in range(n)
    ]
    lines: list[str] = []
    lines.append("# DF-HRS-112 batch-on-the-wire consumer-scale re-measurement")
    lines.append(f"Date: {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    lines.append(
        f"Python: {sys.version.split()[0]}  sqlite3 lib: {sqlite3.sqlite_version}"
    )
    lines.append(f"Adapter binary: {release_bin} (built release profile)")
    lines.append(
        f"Adapter git HEAD: {subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=REPO, capture_output=True, text=True).stdout.strip()}"
    )
    lines.append(f"N={n} messages per protocol, repeats={runs}")
    lines.append(
        "Arms: (A) per-message append_message, one durable commit per call, "
        "over the wire; (B) ONE append_messages_batch call over the wire "
        "(one transaction, like the original stdlib control); "
        "(C) stdlib sqlite3 executemany in ONE transaction (original control)."
    )
    lines.append("")

    def protocol_a() -> float:
        db = os.path.join(scratch, f"bulk-a-{time.time_ns()}.db")
        t0 = time.perf_counter()
        with SessionAdapter(adapter) as p:
            p.open(db, create=True)
            p.ensure_session("bulk", source="bulk", model="m")
            for role, content in body:
                p.append_message("bulk", role=role, content=content)
            p.checkpoint()
            p.close_store()
        return time.perf_counter() - t0

    def protocol_b() -> float:
        db = os.path.join(scratch, f"bulk-b-{time.time_ns()}.db")
        t0 = time.perf_counter()
        with SessionAdapter(adapter) as p:
            p.open(db, create=True)
            p.ensure_session("bulk", source="bulk", model="m")
            p.append_messages_batch(
                "bulk", [{"role": r, "content": c} for r, c in body]
            )
            p.checkpoint()
            p.close_store()
        return time.perf_counter() - t0

    def control_c() -> float:
        db = os.path.join(scratch, f"bulk-c-{time.time_ns()}.db")
        t0 = time.perf_counter()
        con = sqlite3.connect(db)
        try:
            con.execute("PRAGMA journal_mode=WAL")
            con.execute(
                "CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, "
                "session_id TEXT, role TEXT, content TEXT)"
            )
            con.execute("BEGIN IMMEDIATE")
            con.executemany(
                "INSERT INTO messages (session_id, role, content) VALUES (?,?,?)",
                [("bulk", r, c) for r, c in body],
            )
            con.commit()
        finally:
            con.close()
        return time.perf_counter() - t0

    samples = {"A_per_message_wire": [], "B_batch_wire": [], "C_one_txn_sqlite": []}
    fns = {
        "A_per_message_wire": protocol_a,
        "B_batch_wire": protocol_b,
        "C_one_txn_sqlite": control_c,
    }
    try:
        for run in range(1, runs + 1):
            for name, fn in fns.items():
                dt = fn()
                samples[name].append(dt)
                lines.append(f"RUN{run} {name}={dt:.3f}s ({n / dt:.0f} msg/s)")
        lines.append("")
        med_a = sorted(samples["A_per_message_wire"])[
            len(samples["A_per_message_wire"]) // 2
        ]
        med_b = sorted(samples["B_batch_wire"])[len(samples["B_batch_wire"]) // 2]
        med_c = sorted(samples["C_one_txn_sqlite"])[
            len(samples["C_one_txn_sqlite"]) // 2
        ]
        lines.append(f"MEDIAN A_per_message_wire={med_a:.3f}s ({n / med_a:.0f} msg/s)")
        lines.append(f"MEDIAN B_batch_wire={med_b:.3f}s ({n / med_b:.0f} msg/s)")
        lines.append(f"MEDIAN C_one_txn_sqlite={med_c:.3f}s ({n / med_c:.0f} msg/s)")
        lines.append(f"RATIO per_message/batch (wire) = {med_a / med_b:.1f}x")
        lines.append(
            "NOTE: arm A here (one durable commit per append_message RPC) is the "
            "arm the 2026-10-17 bulk-ab.log measured at 27.6s/592.7x; arm B is "
            "the batch API on the wire; arm C is the original stdlib control."
        )
    finally:
        os.makedirs(os.path.dirname(out_path), exist_ok=True)
        with open(out_path, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
        subprocess.run(["rm", "-rf", scratch], check=False)
    print(f"BULK_WIRE_MEASURED n={n} runs={runs} transcript at {out_path}")


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
    ap.add_argument(
        "--mode",
        choices=["proof", "broken", "batch-wire", "bulk-wire"],
        default="proof",
    )
    ap.add_argument("--out", default=None, help="transcript path for --mode broken")
    ap.add_argument(
        "--adapter",
        default=None,
        help="adapter binary path (cargo tests pass a task-unique copy)",
    )
    ap.add_argument(
        "--n",
        type=int,
        default=200,
        help="message count for the batch-wire leg (default 200)",
    )
    ap.add_argument(
        "--bulk-n",
        type=int,
        default=2000,
        help="message count for the bulk-wire measurement (default 2000)",
    )
    ap.add_argument(
        "--bulk-runs",
        type=int,
        default=3,
        help="repeats for the bulk-wire measurement (default 3)",
    )
    ap.add_argument(
        "--bulk-out",
        default=None,
        help="transcript path for --mode bulk-wire (default: docs/evidence/hrs109/)",
    )
    args = ap.parse_args()
    adapter = resolve_adapter(args.adapter)

    if args.mode == "broken":
        if not args.out:
            print("--out is required for --mode broken", file=sys.stderr)
            return 1
        broken_proof(args.out, adapter)
        return 0

    if args.mode == "batch-wire":
        # DF-HRS-112: standalone runs build the release adapter per the task
        # contract; cargo-test runs reuse the provided task-unique copy.
        release = resolve_release_adapter(args.adapter)
        batch_usage_wire_proof(release, n=args.n)
        return 0

    if args.mode == "bulk-wire":
        release = resolve_release_adapter(args.adapter)
        out = args.bulk_out or os.path.join(
            REPO, "docs", "evidence", "hrs109", "batch-2000-wire.log"
        )
        bulk_wire_measurement(release, args.bulk_n, args.bulk_runs, out, release)
        return 0

    scratch = tempfile.mkdtemp(prefix="hrs003-proof-")
    try:
        roundtrip_proof(scratch, adapter)
        # DF-HRS-112: the default proof now also drives append_messages_batch
        # and record_model_usage end-to-end over the same wire.
        batch_adapter = resolve_release_adapter(args.adapter)
        batch_usage_wire_proof(batch_adapter, n=args.n)
    finally:
        subprocess.run(["rm", "-rf", scratch], check=False)
    return 0


if __name__ == "__main__":
    sys.exit(main())
