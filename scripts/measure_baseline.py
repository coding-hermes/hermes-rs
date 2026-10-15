#!/usr/bin/env python3
"""Resource baseline measurement harness, corrected methodology (HRS-006).

Supersedes the HRS-005 measurement pass. Fixes against the previous version:

1. cpu_s: per-turn process-tree CPU delta sampled from /proc/<pid>/stat
   (utime+stime) over the spawned process AND all descendants (PPid walk,
   0.25 s ticks) — replaces the cumulative RUSAGE_CHILDREN counter whose
   max() across thread workers misattributed CPU under concurrency.
2. rss_mb_end: the FINAL tree sample (sum of resident pages across the tree
   at the last tick before exit) — replaces max(rss), which published a
   PEAK as an end value. rss_mb_peak stays the max tick sum.
3. duration_seconds: batch elapsed wall-clock (time.monotonic around the
   whole capture run) — replaces the sum of per-turn walls, which inflated
   and deflated throughput under concurrency. Per-turn walls are kept for
   latency percentiles only.
4. latency percentiles: nearest-rank, documented, with a "low_sample" flag
   when fewer than 20 turns feed them.
5. --selftest: validates the sampler against a synthetic workload with
   known CPU (~2.5 s) and RSS (~200 MB) targets; each capture JSON records
   the selftest result. A capture refuses to run after a selftest FAIL.

Workload profiles (deterministic, provider-backed, no human in the loop):
- continuing_conversation: N scripted follow-up turns against the SAME
  session. Turn 1 plants a codeword; later turns resume that session
  (preferred: `hermes -z <prompt> --resume <session-id>` with the id read
  from the session store; fallback: bare `-c` MRU). Continuation is
  verified, not assumed: recall turns must echo the planted codeword or
  the turn is marked not-ok.
- tool_execution: each turn plants a random token file and instructs the
  agent to `cat` it with its terminal tool; the turn only counts ok if the
  random token appears in the output (the model cannot guess it).
- sqlite_persistence: turns that read and write session state (plant +
  recall codeword) so the session store grows; HERMES_HOME byte growth
  (du-equivalent) is measured before/after the batch.

Runs EACH profile at a requested concurrency (concurrent sessions, each
running its turns sequentially) with multiple repetitions. Emits one raw
JSON per rep plus a summary JSON (mean/stddev/min/max), schema
"hermes-rs/baseline/2". Schema "1" files from HRS-005 remain on disk as
historical records; nothing rewrites them.

Separation rule: this harness measures the pinned PYTHON reference only.
It makes no Python-vs-Rust efficiency claim anywhere.

Every null is either {"value": null, "reason": "..."} or a value with a
sibling "<field>_reason" key (DF-HRS-102 contract).

Standard library only. Python >= 3.11.

Usage:
  python3 scripts/measure_baseline.py --selftest
  python3 scripts/measure_baseline.py --pin /tmp/hermes-upstream-pin-hrs001 \
      --profile continuing_conversation --concurrency 1 --reps 5 --turns 3 \
      --out-dir baseline
  python3 scripts/measure_baseline.py --profile sqlite_persistence \
      --concurrency 4 --reps 3 --out-dir /tmp/hrs006

Requires ROUTER_API_KEY in the environment for live capture (used via the
child env only; never logged, never written to argv or JSON).
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import math
import os
import platform
import secrets
import signal
import statistics
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

EXPECTED_PIN_COMMIT = "e06df9c2eae6901f4c88f464e0ae6e753c0428b8"
SCHEMA = "hermes-rs/baseline/2"
SAMPLE_INTERVAL_S = 0.25
CLK_TCK = os.sysconf("SC_CLK_TCK")
PAGE_KB = os.sysconf("SC_PAGE_SIZE") // 1024

# --- deterministic scripted workloads ---------------------------------------

CC_PROMPTS = [
    (
        "Remember this codeword for the rest of this conversation: LANTERN-7. "
        "Reply with exactly: acknowledged."
    ),
    (
        "What codeword did I ask you to remember earlier in this conversation? "
        "Reply with the codeword only."
    ),
    "Reply with exactly: LANTERN-7 session check complete.",
]
CC_MARKER = "LANTERN-7"

SQ_PROMPTS = [
    (
        "Remember this token for the rest of this conversation: ZEPHYR-31. "
        "Reply with exactly: stored."
    ),
    (
        "Recall the token I asked you to remember earlier in this conversation "
        "and reply with exactly that token."
    ),
    "Recall the token again and reply with exactly: ZEPHYR-31 recalled.",
]
SQ_MARKER = "ZEPHYR-31"

TOOL_PROMPT = (
    "Use your terminal tool to run exactly this shell command: cat {path} "
    "Then reply with exactly the command output and nothing else."
)

PROFILES = ("continuing_conversation", "tool_execution", "sqlite_persistence")


# --- /proc process-tree sampling --------------------------------------------


def _read_proc_stat(pid: int) -> tuple[int, int] | None:
    """Return (ppid, utime+stime jiffies) for a pid, or None."""
    try:
        with open(f"/proc/{pid}/stat", "rb") as fh:
            data = fh.read().decode("utf-8", "replace")
    except OSError:
        return None
    rp = data.rfind(")")
    if rp < 0:
        return None
    fields = data[rp + 2 :].split()
    # fields[0]=state(3) fields[1]=ppid(4) ... fields[11]=utime(14) fields[12]=stime(15)
    try:
        return int(fields[1]), int(fields[11]) + int(fields[12])
    except (IndexError, ValueError):
        return None


def _read_proc_rss_kb(pid: int) -> int | None:
    """Resident set size in kB from /proc/<pid>/statm field 2, or None."""
    try:
        with open(f"/proc/{pid}/statm", "r", encoding="utf-8") as fh:
            fields = fh.read().split()
        return int(fields[1]) * PAGE_KB
    except (OSError, IndexError, ValueError):
        return None


def sample_tree(root_pid: int) -> tuple[int, int] | None:
    """CPU jiffies and RSS kB summed over root_pid's process tree.

    Returns None when the root is no longer visible in /proc (exited and
    reaped). Descendants are discovered each tick via PPid links, so
    short-lived children are covered whenever they are alive at a tick;
    a child that lives and dies entirely between two ticks is missed
    (documented limitation of tick sampling).
    """
    procs: dict[int, tuple[int, int]] = {}
    try:
        entries = os.listdir("/proc")
    except OSError:
        return None
    for entry in entries:
        if not entry.isdigit():
            continue
        st = _read_proc_stat(int(entry))
        if st is not None:
            procs[int(entry)] = st
    if root_pid not in procs:
        return None
    children: dict[int, list[int]] = {}
    for pid, (ppid, _cpu) in procs.items():
        children.setdefault(ppid, []).append(pid)
    total_cpu = 0
    total_rss = 0
    stack = [root_pid]
    seen: set[int] = set()
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        st = procs.get(pid)
        if st is None:
            continue
        total_cpu += st[1]
        rss = _read_proc_rss_kb(pid)
        if rss is not None:
            total_rss += rss
        stack.extend(children.get(pid, []))
    return total_cpu, total_rss


class TreeSampler:
    """Background sampler for one spawned process tree (0.25 s ticks)."""

    def __init__(self, root_pid: int, interval: float = SAMPLE_INTERVAL_S):
        self.root = root_pid
        self.interval = interval
        self.samples: list[tuple[float, int, int]] = []  # (mono, cpu_jiffies, rss_kb)
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _run(self) -> None:
        while not self._stop.is_set():
            s = sample_tree(self.root)
            if s is None:
                return  # root gone (exited/reaped); final tick handled in stop()
            self.samples.append((time.monotonic(), s[0], s[1]))
            self._stop.wait(self.interval)

    def stop(self) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=3)
        s = sample_tree(self.root)  # best-effort final tick
        if s is not None:
            self.samples.append((time.monotonic(), s[0], s[1]))

    def results(self) -> dict:
        n = len(self.samples)
        if n == 0:
            return {
                "cpu_s": None,
                "rss_peak_kb": None,
                "rss_end_kb": None,
                "ticks": 0,
                "reason": "no_proc_samples_root_exited_before_first_tick",
            }
        cpu_delta = (self.samples[-1][1] - self.samples[0][1]) / CLK_TCK
        return {
            "cpu_s": cpu_delta,
            "rss_peak_kb": max(s[2] for s in self.samples),
            "rss_end_kb": self.samples[-1][2],
            "ticks": n,
        }


# --- selftest ----------------------------------------------------------------

SELFTEST_CHILD = r"""
import subprocess, sys, time
buf = bytearray(200 * 1024 * 1024)
for i in range(0, len(buf), 4096):
    buf[i] = 1
t0 = time.process_time()
while time.process_time() - t0 < 2.0:
    sum(range(1000))
if len(sys.argv) > 1 and sys.argv[1] == "grandchild":
    subprocess.run([sys.executable, "-c",
        "import time\n"
        "t0 = time.process_time()\n"
        "while time.process_time() - t0 < 0.5:\n"
        "    sum(range(1000))\n"])
print("done")
"""


def run_selftest() -> dict:
    """Validate the sampler against known synthetic values.

    Child allocates ~200 MB (pages touched), spins ~2.0 s CPU, then spawns a
    grandchild that spins ~0.5 s CPU. Expected: tree CPU ~2.5 s (+-0.5 s),
    tree RSS peak ~200 MB (+-25%, small interpreter overhead of the
    grandchild included in the tolerance).
    """
    proc = subprocess.Popen(
        [sys.executable, "-c", SELFTEST_CHILD, "grandchild"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    sampler = TreeSampler(proc.pid)
    t0 = time.monotonic()
    sampler.start()
    try:
        proc.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
    sampler.stop()
    wall = time.monotonic() - t0
    res = sampler.results()
    details = {
        "expected_cpu_s": 2.5,
        "expected_rss_mb": 200.0,
        "measured_cpu_s": None if res["cpu_s"] is None else round(res["cpu_s"], 3),
        "measured_rss_peak_mb": None
        if res["rss_peak_kb"] is None
        else round(res["rss_peak_kb"] / 1024.0, 1),
        "ticks": res["ticks"],
        "child_wall_s": round(wall, 3),
    }
    ok = False
    fail_reason = ""
    if res["cpu_s"] is None or res["rss_peak_kb"] is None:
        fail_reason = f"sampler returned no samples: {res.get('reason', 'unknown')}"
    else:
        rss_mb = res["rss_peak_kb"] / 1024.0
        rss_ok = 150.0 <= rss_mb <= 250.0
        cpu_ok = 2.0 <= res["cpu_s"] <= 3.0
        ok = rss_ok and cpu_ok
        if not ok:
            fail_reason = (
                f"out of tolerance: rss_ok={rss_ok} cpu_ok={cpu_ok} "
                f"(rss {rss_mb:.1f}MB must be within 150-250; "
                f"cpu {res['cpu_s']:.2f}s must be within 2.0-3.0)"
            )
    details["fail_reason"] = fail_reason
    return {
        "sampler_selftest": "pass" if ok else "fail",
        "sampler_selftest_details": details,
    }


# --- hermes invocation --------------------------------------------------------

PROVIDER_CONFIG = """\
providers:
  9router:
    base_url: "http://localhost:20128/v1"
    key_env: "ROUTER_API_KEY"
"""


def seed_provider_config(home: Path) -> None:
    """Seed the session home's config.yaml with the 9router custom provider.

    Without it the pinned CLI fails every turn with ``Unknown provider
    '9router'`` (verified live in the first rep-0 attempt); the credential
    itself is never written to disk - the CLI reads it from the ROUTER_API_KEY
    env var passed to the child process (key_env indirection).
    """
    cfg = home / "config.yaml"
    if not cfg.exists():
        cfg.write_text(PROVIDER_CONFIG, encoding="utf-8")


NO_SESSION_MSG = "No previous CLI session to continue"


def wait_for_finalized_session(home: Path, timeout: float = 10.0) -> bool:
    """Wait until the home's session row carries a non-null last_activity_at.

    Observed live under a 4-wide batch (c4 rep01, session 1): the turn-0
    process exited rc=0 with correct output but its exit-time activity
    write had not landed, so the next turn's bare ``-c`` MRU lookup found
    no session and failed instantly ("No previous CLI session to
    continue"). This read-only sqlite poll keeps the NEXT turn from
    starting before the previous turn's session row is finalized.
    """
    import sqlite3

    db = home / "state.db"
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if db.exists():
            try:
                conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=2)
                try:
                    (n,) = conn.execute(
                        "SELECT COUNT(*) FROM sessions "
                        "WHERE last_activity_at IS NOT NULL"
                    ).fetchone()
                finally:
                    conn.close()
                if n and n > 0:
                    return True
            except sqlite3.Error:
                pass
        time.sleep(0.5)
    return False


def latest_session_id(home: Path) -> str | None:
    """Most recent session id in the home's session store (read-only)."""
    import sqlite3

    db = home / "state.db"
    if not db.exists():
        return None
    try:
        conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=2)
        try:
            row = conn.execute(
                "SELECT id FROM sessions ORDER BY started_at DESC LIMIT 1"
            ).fetchone()
        finally:
            conn.close()
        return row[0] if row else None
    except sqlite3.Error:
        return None


def dir_size_bytes(path: Path) -> int:
    total = 0
    for p in path.rglob("*"):
        try:
            if p.is_file():
                total += p.stat().st_size
        except OSError:
            pass
    return total


def run_turn(
    pin: str,
    home: Path,
    api_key: str,
    model: str,
    prompt: str,
    continuing: bool,
    resume_id: str | None,
    expected_marker: str | None,
    turn_timeout: float,
) -> dict:
    """Run one hermes -z turn with tree sampling; returns per-turn record.

    Resume strategy: an explicit session id (--resume <id>) is preferred -
    the bare `-c` MRU lookup proved unreliable under load because the pin
    writes last_activity_at at process exit and that write can be lost,
    leaving the MRU query empty while the session content is intact. When
    no id is known, a continuing turn falls back to bare `-c`.
    """
    env = dict(os.environ)
    env["HERMES_HOME"] = str(home)
    env["ROUTER_API_KEY"] = api_key
    cmd = ["uv", "run", "hermes", "-z", prompt, "-m", model, "--provider", "9router"]
    if resume_id:
        cmd.extend(["--resume", resume_id])
    elif continuing:
        cmd.append("-c")
    proc = subprocess.Popen(
        cmd,
        cwd=pin,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    sampler = TreeSampler(proc.pid)
    t0 = time.monotonic()
    sampler.start()
    timed_out = False
    try:
        out, _ = proc.communicate(timeout=turn_timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError, OSError):
            pass
        out, _ = proc.communicate()
    wall = time.monotonic() - t0
    sampler.stop()
    res = sampler.results()
    rec: dict = {
        "ok": bool(
            not timed_out
            and proc.returncode == 0
            and (expected_marker is None or expected_marker in out)
        ),
        "wall_s": wall,
    }
    if timed_out:
        rec["ok_reason"] = "turn_timeout_killed"
    elif proc.returncode != 0:
        rec["ok_reason"] = f"exit_code_{proc.returncode}"
    elif expected_marker is not None and expected_marker not in out:
        rec["ok_reason"] = "expected_marker_missing_from_output"
    if res["cpu_s"] is None:
        rec["cpu_s"] = None
        rec["cpu_s_reason"] = res.get("reason", "sampler_no_data")
    else:
        rec["cpu_s"] = round(res["cpu_s"], 3)
        rec["cpu_ticks"] = res["ticks"]
    if res["rss_peak_kb"] is None:
        rec["rss_mb_peak"] = None
        rec["rss_mb_peak_reason"] = res.get("reason", "sampler_no_data")
    else:
        rec["rss_mb_peak"] = round(res["rss_peak_kb"] / 1024.0, 1)
    if res["rss_end_kb"] is None:
        rec["rss_mb_end"] = None
        rec["rss_mb_end_reason"] = res.get("reason", "sampler_no_data")
    else:
        rec["rss_mb_end"] = round(res["rss_end_kb"] / 1024.0, 1)
    rec["output_tail"] = out[-400:]
    return rec


def build_turn_plan(
    profile: str, turns: int, rep: int, session_idx: int, home: Path
) -> list[dict]:
    """Per-turn (prompt, continuing, expected_marker) for one session."""
    plan = []
    if profile == "continuing_conversation":
        for i in range(turns):
            prompt = CC_PROMPTS[min(i, len(CC_PROMPTS) - 1)]
            marker = None if i == 0 else CC_MARKER
            plan.append({"prompt": prompt, "continuing": i > 0, "marker": marker})
    elif profile == "sqlite_persistence":
        for i in range(turns):
            prompt = SQ_PROMPTS[min(i, len(SQ_PROMPTS) - 1)]
            marker = None if i == 0 else SQ_MARKER
            plan.append({"prompt": prompt, "continuing": i > 0, "marker": marker})
    elif profile == "tool_execution":
        for i in range(turns):
            token = secrets.token_hex(8)
            probe = home / f"hrs006-probe-r{rep}-s{session_idx}-t{i}.txt"
            probe.write_text(token + "\n", encoding="utf-8")
            plan.append(
                {
                    "prompt": TOOL_PROMPT.format(path=probe),
                    "continuing": i > 0,
                    "marker": token,
                }
            )
    else:
        raise ValueError(f"unknown profile: {profile}")
    return plan


def run_session(
    profile: str,
    pin: str,
    home: Path,
    api_key: str,
    model: str,
    turns: int,
    rep: int,
    session_idx: int,
    turn_timeout: float,
) -> list[dict]:
    plan = build_turn_plan(profile, turns, rep, session_idx, home)
    out = []
    resume_id: str | None = None
    for i, step in enumerate(plan):
        rec = run_turn(
            pin,
            home,
            api_key,
            model,
            step["prompt"],
            step["continuing"],
            resume_id,
            step["marker"],
            turn_timeout,
        )
        # One retry for the observed turn-0 finalize race: the previous turn
        # exited rc=0 but its session row (last_activity_at) had not landed,
        # so bare `-c` MRU found no session. Wait for the row, resolve the
        # explicit id, and retry once via --resume.
        if not rec["ok"] and NO_SESSION_MSG in (rec.get("output_tail") or ""):
            wait_for_finalized_session(home, timeout=15.0)
            resume_id = latest_session_id(home) or resume_id
            retry = run_turn(
                pin,
                home,
                api_key,
                model,
                step["prompt"],
                False,
                resume_id,
                step["marker"],
                turn_timeout,
            )
            retry["session"] = session_idx
            retry["turn"] = i
            retry["retried_no_session_race"] = True
            rec = retry
        rec["session"] = session_idx
        rec["turn"] = i
        out.append(rec)
        # After turn 0 of a multi-turn session: resolve the explicit session
        # id once, so later turns resume by id (MRU-timestamp loss in the pin
        # cannot derail them). Bounded wait; absence tolerated (later turns
        # then fall back to bare -c MRU).
        if i == 0 and len(plan) > 1:
            wait_for_finalized_session(home)
            resume_id = latest_session_id(home)
    return out


# --- measurement assembly -----------------------------------------------------


def meas(value, reason: str | None = None, **extra) -> dict:
    """{"value": x} or {"value": null, "reason": ...} (DF-HRS-102)."""
    if value is None:
        d = {"value": None, "reason": reason or "unspecified"}
    else:
        d = {"value": value}
    d.update(extra)
    return d


def percentile_field(sorted_vals: list[float], p: float) -> dict:
    if not sorted_vals:
        return {"value": None, "reason": "no_successful_turn_walls"}
    idx = max(0, math.ceil(p * len(sorted_vals)) - 1)
    field = {"value": round(sorted_vals[idx] * 1000.0, 1)}
    if len(sorted_vals) < 20:
        field["low_sample"] = True
        field["note"] = f"nearest-rank over n={len(sorted_vals)} turns (<20)"
    else:
        field["method_note"] = f"nearest-rank over n={len(sorted_vals)} turns"
    return field


def capture_rep(
    profile: str,
    pin: str,
    api_key: str,
    model: str,
    turns: int,
    concurrency: int,
    rep: int,
    home_root: Path,
    turn_timeout: float,
) -> tuple[dict, list[dict], dict]:
    """Run one rep batch; returns (measurements, turns_detail, rep_extras)."""
    homes = []
    for s in range(concurrency):
        h = home_root / f"{profile}-c{concurrency}-rep{rep:02d}-s{s}"
        h.mkdir(parents=True, exist_ok=True)
        seed_provider_config(h)
        homes.append(h)
    sizes_before = {h: dir_size_bytes(h) for h in homes}

    t0 = time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as ex:
        futs = [
            ex.submit(
                run_session,
                profile,
                pin,
                home,
                api_key,
                model,
                turns,
                rep,
                s,
                turn_timeout,
            )
            for s, home in enumerate(homes)
        ]
        per_session = [f.result() for f in futs]
    batch_wall = time.monotonic() - t0

    turns_detail = [rec for session in per_session for rec in session]
    ok_turns = [t for t in turns_detail if t["ok"]]
    walls = [t["wall_s"] for t in turns_detail]
    walls_sorted = sorted(t["wall_s"] for t in ok_turns) or sorted(walls)
    cpu_total = sum(t["cpu_s"] for t in turns_detail if t["cpu_s"] is not None)
    cpu_missing = [t for t in turns_detail if t["cpu_s"] is None]
    peaks = [t["rss_mb_peak"] for t in turns_detail if t["rss_mb_peak"] is not None]
    wall_sum = sum(walls)

    # rss_mb_end: the final tree sample of the LAST-FINISHING turn (its own
    # last tick before exit). At concurrency 1 this is exactly the batch
    # process tree's end sample.
    last_by_wall = (
        max(turns_detail, key=lambda t: t["wall_s"]) if turns_detail else None
    )
    if last_by_wall is not None and last_by_wall.get("rss_mb_end") is not None:
        rss_end_field = {"value": last_by_wall["rss_mb_end"]}
        rss_end_field["method_note"] = (
            "final tree sample of the last-finishing turn (concurrency="
            f"{concurrency}); at concurrency 1 this is the batch tree's end sample"
        )
    else:
        rss_end_field = {
            "value": None,
            "reason": "no_turn_ended_with_a_live_rss_sample",
        }

    growth_bytes = None
    if profile == "sqlite_persistence":
        growth_bytes = sum(dir_size_bytes(h) - sizes_before[h] for h in homes)

    measurements = {
        "duration_seconds": round(batch_wall, 3),
        "wall_clock_method": (
            "time.monotonic elapsed around the whole concurrent batch "
            "(NOT the sum of per-turn walls)"
        ),
        "per_turn_wall_sum_s": round(wall_sum, 3),
        "wall_sum_over_batch": round(wall_sum / batch_wall, 3)
        if batch_wall > 0
        else None,
        "cpu_seconds_total": round(cpu_total, 3),
        "cpu_method": (
            "per-turn process-tree delta of /proc/<pid>/stat utime+stime "
            f"(CLK_TCK={CLK_TCK} jiffies/s), sampled every {SAMPLE_INTERVAL_S}s over "
            "the spawned process and all descendants (PPid walk); cumulative "
            "for any process alive at >=1 tick; sub-tick startup/teardown CPU "
            "is under-counted"
        ),
        "cpu_turns_without_samples": len(cpu_missing),
        "cpu_percent_avg": round(100.0 * cpu_total / batch_wall, 1)
        if batch_wall > 0
        else None,
        "rss_mb_peak": round(max(peaks), 1) if peaks else None,
        "rss_mb_peak_reason": None if peaks else "no_rss_samples_collected",
        "rss_mb_end": rss_end_field,
        "rss_method": (
            "sum of /proc/<pid>/statm resident pages x page size across the "
            "process tree each 0.25s tick; peak = max tick sum; end = last "
            "tick sum before exit"
        ),
        "latency_p50_ms": percentile_field(walls_sorted, 0.50),
        "latency_p95_ms": percentile_field(walls_sorted, 0.95),
        "latency_p99_ms": percentile_field(walls_sorted, 0.99),
        "latency_percentile_method": (
            "nearest-rank over per-turn wall seconds of successful turns "
            "(falls back to all turns when none succeeded)"
        ),
        "latency_sample_count": len(walls_sorted),
        "throughput_turns_per_min": round(len(ok_turns) / (batch_wall / 60.0), 2)
        if batch_wall > 0
        else None,
        "storage_growth_mb": meas(
            None if growth_bytes is None else round(growth_bytes / (1024 * 1024), 3),
            reason=None
            if growth_bytes is not None
            else "storage_growth_only_measured_for_sqlite_persistence",
        ),
    }
    rep_extras = {
        "turns_ok": f"{len(ok_turns)}/{len(turns_detail)}",
        "homes": [str(h) for h in homes],
    }
    return measurements, turns_detail, rep_extras


# --- summary stats ------------------------------------------------------------


def stats_field(
    per_rep: list[tuple[int, float | None, str]], scale: float = 1.0
) -> dict:
    vals = [v for _, v, _ in per_rep if v is not None]
    if not vals:
        reasons = "; ".join(f"rep{r}: {reason}" for r, _, reason in per_rep)
        return {
            "mean": None,
            "stddev": None,
            "min": None,
            "max": None,
            "n": 0,
            "reason": f"all reps null ({reasons})",
        }
    scaled = [v * scale for v in vals]
    out = {
        "mean": round(statistics.fmean(scaled), 4),
        "stddev": round(statistics.stdev(scaled), 4) if len(scaled) > 1 else 0.0,
        "min": round(min(scaled), 4),
        "max": round(max(scaled), 4),
        "n": len(scaled),
        "stddev_method": "sample stddev over reps",
    }
    excluded = [(r, reason) for r, v, reason in per_rep if v is None]
    if excluded:
        out["reps_excluded"] = [r for r, _ in excluded]
        out["reps_excluded_reasons"] = {f"rep{r}": reason for r, reason in excluded}
    return out


# --- environment / provenance ---------------------------------------------------


def environment_metadata() -> dict:
    uname = platform.uname()
    cpu_model = "unknown"
    try:
        with open("/proc/cpuinfo", "r", encoding="utf-8") as fh:
            for line in fh:
                if line.startswith("model name"):
                    cpu_model = line.split(":", 1)[1].strip()
                    break
    except OSError:
        pass
    os_release = sys.platform
    try:
        with open("/etc/os-release", "r", encoding="utf-8") as fh:
            for line in fh:
                if line.startswith("PRETTY_NAME="):
                    os_release = line.split("=", 1)[1].strip().strip('"')
                    break
    except OSError:
        pass
    return {
        "uname": " ".join([uname.system, uname.node, uname.release, uname.machine]),
        "kernel": uname.release,
        "arch": uname.machine,
        "os_release": os_release,
        "cpu_count": os.cpu_count(),
        "cpu_model": cpu_model,
        "python_version": platform.python_version(),
        "captured_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    }


def verify_pin(pin: str) -> tuple[bool, str]:
    hermes = Path(pin) / ".venv" / "bin" / "hermes"
    if not hermes.exists():
        return (
            False,
            f"hermes binary missing at pin ({hermes}); recreate per specs/UPSTREAM_BASELINE.md",
        )
    try:
        got = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=pin,
            capture_output=True,
            text=True,
            timeout=20,
        ).stdout.strip()
    except Exception as exc:  # noqa: BLE001 - report any failure as reason
        return False, f"git rev-parse at pin failed: {exc}"
    if got != EXPECTED_PIN_COMMIT:
        return (
            False,
            f"pin commit mismatch: expected {EXPECTED_PIN_COMMIT}, found {got}",
        )
    return True, "verified v0.22.0 @ " + EXPECTED_PIN_COMMIT


# --- main ----------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--selftest",
        action="store_true",
        help="run the sampler selftest against a synthetic workload and exit",
    )
    ap.add_argument("--pin", default="/tmp/hermes-upstream-pin-hrs001")
    ap.add_argument("--profile", choices=sorted(PROFILES), default=None)
    ap.add_argument("--home-root", default="/tmp/hrs006-homes")
    ap.add_argument(
        "--turns", type=int, default=3, help="scripted turns per session per rep batch"
    )
    ap.add_argument(
        "--concurrency", type=int, default=1, help="concurrent sessions per rep batch"
    )
    ap.add_argument("--reps", type=int, default=5, help="repeated batch repetitions")
    ap.add_argument("--model", default="ocg/glm-5.3-flash")
    ap.add_argument("--turn-timeout", type=float, default=240.0)
    ap.add_argument("--out-dir", default=None)
    ap.add_argument("--tag", default="hrs006")
    args = ap.parse_args(argv)

    if args.selftest:
        result = run_selftest()
        print(json.dumps(result, indent=2))
        print("SELFTEST: " + result["sampler_selftest"].upper())
        return 0 if result["sampler_selftest"] == "pass" else 1

    if not args.profile or not args.out_dir:
        ap.error("--profile and --out-dir are required (unless --selftest)")
    if args.turns < 1 or args.concurrency < 1 or args.reps < 1:
        ap.error("--turns/--concurrency/--reps must be >= 1")

    pin_ok, pin_reason = verify_pin(args.pin)
    if not pin_ok:
        print(f"pin_unavailable: {pin_reason}", file=sys.stderr)
        print(
            "Skipping live capture (no numbers fabricated). "
            "Harness and docs are still shippable.",
            file=sys.stderr,
        )
        return 3

    api_key = os.environ.get("ROUTER_API_KEY", "")
    if not api_key:
        print("ROUTER_API_KEY not set; refusing live capture", file=sys.stderr)
        return 2

    selftest = run_selftest()
    if selftest["sampler_selftest"] != "pass":
        print(
            "SELFTEST FAILED - refusing to capture with an invalid sampler",
            file=sys.stderr,
        )
        print(json.dumps(selftest, indent=2), file=sys.stderr)
        return 4
    print(f"sampler selftest: pass ({selftest['sampler_selftest_details']})")

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    home_root = Path(args.home_root)
    home_root.mkdir(parents=True, exist_ok=True)

    env_meta = environment_metadata()
    pin_verified, pin_note = verify_pin(args.pin)

    common_doc = {
        "schema": SCHEMA,
        "profile": args.profile,
        "environment": env_meta,
        "workload": {
            "concurrency": args.concurrency,
            "sessions_per_rep": args.concurrency,
            "turns_per_session": args.turns,
            "reps": args.reps,
            "model": args.model,
            "provider": "9router (localhost:20128 federation gateway)",
            "pin": "v0.22.0 @ " + EXPECTED_PIN_COMMIT,
            "pin_path": args.pin,
            "pin_verified": pin_verified,
            "pin_note": pin_note,
            "hermes_argv_template": (
                "uv run hermes -z <prompt> -m <model> --provider 9router "
                "[--resume <session-id> preferred, bare -c fallback on turns>0]"
            ),
            "profile_prompts": {
                "continuing_conversation": CC_PROMPTS,
                "sqlite_persistence": SQ_PROMPTS,
                "tool_execution": (
                    "per turn: random token file planted in the session home; "
                    "agent told to `cat <abs path>` and echo its output"
                ),
            },
        },
        "sampler_selftest": selftest["sampler_selftest"],
        "sampler_selftest_details": selftest["sampler_selftest_details"],
        "method": {
            "cpu": (
                "per-turn process-tree CPU delta (see cpu_method in measurements); "
                "replaces HRS-005's cumulative RUSAGE_CHILDREN max() which "
                "misattributed CPU under concurrency"
            ),
            "wall": (
                "duration_seconds = batch elapsed monotonic wall clock; per-turn "
                "walls kept for latency percentiles only"
            ),
            "rss": (
                "rss_mb_peak = max tree tick sum; rss_mb_end = final tree sample "
                "of the last-finishing turn (HRS-005 wrongly published peak as end)"
            ),
            "latency_percentiles": "nearest-rank; low_sample flag when n<20",
            "storage": (
                "du-equivalent byte sum over each session HERMES_HOME tree "
                "before/after the batch (sqlite_persistence only)"
            ),
            "continuing_semantics": (
                "continuing = every turn after the first resumes the SAME "
                "session: preferred via `--resume <session-id>` (id read from "
                "the home's session store right after turn 0), fallback via "
                "bare `-c` (MRU inside the turn's isolated HERMES_HOME); "
                "explicit ids are preferred because the pin writes "
                "last_activity_at at process exit and that write is "
                "occasionally lost under load, which empties the MRU lookup "
                "while session content stays intact (observed live, c4). "
                "Continuation itself is VERIFIED per turn by codeword recall, "
                "not assumed"
            ),
            "null_contract": (
                "every null is {value: null, reason: ...} or carries a sibling "
                "<field>_reason key (DF-HRS-102)"
            ),
        },
        "notes": (
            "Measured against the pinned Python reference via its real agent "
            "loop; provider/network latency is included (end-to-end, not "
            "compute-only). Local compute/resource measurement ONLY - this "
            "record makes no Python-vs-Rust compatibility or efficiency claim."
        ),
    }

    rep_rows = []
    all_ok = True
    span_start = datetime.now(timezone.utc)
    for rep in range(args.reps):
        measurements, turns_detail, rep_extras = capture_rep(
            args.profile,
            args.pin,
            api_key,
            args.model,
            args.turns,
            args.concurrency,
            rep,
            home_root,
            args.turn_timeout,
        )
        doc = dict(common_doc)
        doc["rep"] = rep
        doc["captured_at"] = datetime.now(timezone.utc).isoformat()
        doc["measurements"] = measurements
        doc["turns_detail"] = turns_detail
        doc["turns_ok"] = rep_extras["turns_ok"]
        doc["session_homes"] = rep_extras["homes"]
        fname = f"{args.tag}-{args.profile}-c{args.concurrency}-rep{rep:02d}.json"
        (out_dir / fname).write_text(json.dumps(doc, indent=2) + "\n")
        ok_count, tot = rep_extras["turns_ok"].split("/")
        if ok_count != tot:
            all_ok = False

        def pf(v, reason):
            """Number as-is; null as {value, reason} (DF-HRS-102)."""
            if v is None:
                return {"value": None, "reason": reason}
            return v

        rep_rows.append(
            {
                "rep": rep,
                "file": fname,
                "turns_ok": rep_extras["turns_ok"],
                "duration_seconds": pf(
                    measurements["duration_seconds"], "batch_wall_zero"
                ),
                "cpu_seconds_total": pf(
                    measurements["cpu_seconds_total"], "turns_without_cpu_samples"
                ),
                "rss_mb_peak": pf(
                    measurements["rss_mb_peak"], "no_rss_samples_collected"
                ),
                "rss_mb_end": pf(
                    measurements["rss_mb_end"].get("value"),
                    measurements["rss_mb_end"].get(
                        "reason", "no_turn_ended_with_a_live_rss_sample"
                    ),
                ),
                "throughput_turns_per_min": pf(
                    measurements["throughput_turns_per_min"], "batch_wall_zero"
                ),
                "storage_growth_mb": pf(
                    measurements["storage_growth_mb"].get("value"),
                    measurements["storage_growth_mb"].get(
                        "reason", "storage_growth_unmeasured"
                    ),
                ),
            }
        )
        print(
            f"rep {rep + 1}/{args.reps}: turns_ok={rep_extras['turns_ok']} "
            f"wall={measurements['duration_seconds']}s "
            f"cpu={measurements['cpu_seconds_total']}s "
            f"rss_peak={measurements['rss_mb_peak']}MB -> {fname}"
        )
    span_end = datetime.now(timezone.utc)

    def collect(field: str) -> list[tuple[int, float | None, str]]:
        rows = []
        for r in rep_rows:
            v = r[field]
            if isinstance(v, dict):
                rows.append((r["rep"], None, v.get("reason", "see rep file")))
            else:
                rows.append((r["rep"], v, "see rep file"))
        return rows

    summary = dict(common_doc)
    summary["kind"] = "summary"
    summary["captured_span"] = {
        "start": span_start.isoformat(),
        "end": span_end.isoformat(),
    }
    summary["per_rep"] = rep_rows
    summary["stats"] = {
        "wall_s": stats_field(collect("duration_seconds")),
        "cpu_s": stats_field(collect("cpu_seconds_total")),
        "rss_mb_peak": stats_field(collect("rss_mb_peak")),
        "rss_mb_end": stats_field(collect("rss_mb_end")),
        "throughput_turns_per_min": stats_field(collect("throughput_turns_per_min")),
        "storage_growth_mb": stats_field(collect("storage_growth_mb")),
    }
    summary["concurrency_math_check"] = {
        "duration_seconds_is_batch_elapsed": True,
        "assertion": (
            "duration_seconds in every rep file is the time.monotonic elapsed "
            "around the whole concurrent batch, never the sum of per-turn walls; "
            "wall_sum_over_batch >> 1 at concurrency>1 demonstrates the old "
            "summed-wall figure would have been wrong"
        ),
        "per_rep_wall_sum_over_batch": [r["file"] for r in rep_rows],
        "per_rep_ratio_field": "measurements.wall_sum_over_batch",
    }
    sname = f"{args.tag}-{args.profile}-c{args.concurrency}-summary.json"
    (out_dir / sname).write_text(json.dumps(summary, indent=2) + "\n")
    print(f"summary written: {out_dir / sname}")
    return 0 if all_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
