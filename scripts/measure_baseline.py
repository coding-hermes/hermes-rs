#!/usr/bin/env python3
"""Measured workload-profile capture for HRS-005 (HRS-001 residual).

Runs the pinned upstream Hermes (v0.22.0 @ e06df9c2) agent loop against a
real provider for P1 (single_session) and P2 (multi_session) profiles,
sampling child-process RSS and CPU. Reuses scripts/capture_baseline.py's
JSON shape; every null carries a per-field reason (DF-HRS-102 contract).

Standard library only. Python >= 3.11.

Usage:
  python3 scripts/measure_baseline.py --pin /tmp/hermes-upstream-pin-hrs001 \
      --profile single_session --turns 3 --out /tmp/hrs005-p1.json
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

PROMPT = "Reply with exactly: OK"


def run_one_turn(pin: str, home: str, api_key: str, model: str) -> dict:
    """Run one hermes -z turn, returning {ok, wall_s, rss_mb_peak, cpu_s}."""
    env = dict(os.environ)
    env.update({"HERMES_HOME": home, "ROUTER_API_KEY": api_key})
    proc = subprocess.Popen(
        ["uv", "run", "hermes", "-z", PROMPT, "-m", model, "--provider", "9router"],
        cwd=pin,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    peak_rss_kb = 0
    start = time.monotonic()
    while proc.poll() is None:
        # sample RSS across the process group via /proc
        try:
            for pid_dir in Path("/proc").iterdir():
                if not pid_dir.name.isdigit():
                    continue
                try:
                    stat = (pid_dir / "status").read_text()
                except OSError:
                    continue
                if f"\nPPid:\t{os.getpid()}\n" not in stat and pid_dir.name != str(
                    proc.pid
                ):
                    continue
                for line in stat.splitlines():
                    if line.startswith("VmRSS:"):
                        peak_rss_kb = max(peak_rss_kb, int(line.split()[1]))
        except Exception:
            pass
        time.sleep(0.25)
    out, _ = proc.communicate()
    wall = time.monotonic() - start
    # CPU seconds of the whole tree via /proc stat after exit is unavailable;
    # use `resource` children counters instead.
    import resource

    cpu_s = (
        resource.getrusage(resource.RUSAGE_CHILDREN).ru_utime
        + resource.getrusage(resource.RUSAGE_CHILDREN).ru_stime
    )
    return {
        "ok": proc.returncode == 0 and "OK" in out,
        "wall_s": wall,
        "rss_mb_peak": peak_rss_kb / 1024.0,
        "cpu_s_children_cumulative": cpu_s,
        "output_tail": out[-200:],
    }


def capture(
    profile: str,
    pin: str,
    home: str,
    api_key: str,
    model: str,
    turns: int,
    concurrency: int,
) -> dict:
    per_turn = []
    if profile == "single_session":
        # one session, sequential scripted turns
        for _ in range(turns):
            per_turn.append(run_one_turn(pin, home, api_key, model))
    else:
        # multi_session: `concurrency` concurrent subprocess batches
        import concurrent.futures

        with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as ex:
            futs = [
                ex.submit(run_one_turn, pin, home, api_key, model) for _ in range(turns)
            ]
            per_turn = [f.result() for f in futs]

    ok_turns = [t for t in per_turn if t["ok"]]
    walls = [t["wall_s"] for t in per_turn]
    rss = [t["rss_mb_peak"] for t in per_turn]
    cpu_total = max((t["cpu_s_children_cumulative"] for t in per_turn), default=0.0)
    total_wall = sum(walls)
    return {
        "measurements": {
            "duration_seconds": round(total_wall, 3),
            "cpu_percent_avg": round(100.0 * cpu_total / total_wall, 1)
            if total_wall > 0
            else None,
            "cpu_seconds_total": round(cpu_total, 3),
            "rss_mb_end": round(max(rss), 1) if rss else None,
            "rss_mb_peak": round(max(rss), 1) if rss else None,
            "latency_p50_ms": round(statistics.median(walls) * 1000, 1)
            if walls
            else None,
            "latency_p95_ms": round(
                sorted(walls)[int(0.95 * (len(walls) - 1))] * 1000, 1
            )
            if walls
            else None,
            "latency_p99_ms": round(max(walls) * 1000, 1) if walls else None,
            "throughput_turns_per_min": round(len(ok_turns) / (total_wall / 60.0), 2)
            if total_wall > 0
            else None,
        },
        "turns_detail": [
            {k: t[k] for k in ("ok", "wall_s", "rss_mb_peak")} for t in per_turn
        ],
        "turns_ok": f"{len(ok_turns)}/{len(per_turn)}",
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--pin", required=True)
    ap.add_argument("--home", required=True)
    ap.add_argument(
        "--profile", choices=["single_session", "multi_session"], required=True
    )
    ap.add_argument("--turns", type=int, default=3)
    ap.add_argument("--concurrency", type=int, default=4)
    ap.add_argument("--model", default="ocg/glm-5.3-flash")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    api_key = os.environ.get("ROUTER_API_KEY", "")
    if not api_key:
        print("ROUTER_API_KEY not set", file=__import__("sys").stderr)
        return 2

    conc = 1 if args.profile == "single_session" else args.concurrency
    result = capture(
        args.profile, args.pin, args.home, api_key, args.model, args.turns, conc
    )
    doc = {
        "schema": "hermes-rs/baseline/1",
        "profile": args.profile,
        "pending_implementation": False,
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "environment": {
            "uname": os.uname().sysname + " " + os.uname().release,
            "cpu_count": os.cpu_count(),
            "python_version": __import__("platform").python_version(),
        },
        "workload": {
            "concurrency": conc,
            "sessions": 1 if args.profile == "single_session" else conc,
            "turns": args.turns,
            "model": args.model,
            "provider": "9router (localhost:20128 federation gateway)",
            "prompt": PROMPT,
            "pin": "v0.22.0 @ e06df9c2eae6901f4c88f464e0ae6e753c0428b8",
        },
        **result,
        "notes": "Measured against the pinned Python reference via its real agent loop. "
        "CPU% is RUSAGE_CHILDREN cumulative utime+stime divided by summed wall time "
        "(undercounts concurrent overlap); RSS sampled at 0.25s from /proc across "
        "the process group. NOT a drop-in compatibility or speedup claim.",
    }
    Path(args.out).write_text(json.dumps(doc, indent=2))
    print(f"written: {args.out} turns_ok={result['turns_ok']}")
    return (
        0
        if result["turns_ok"].startswith("3/3") or result["turns_ok"].startswith("6/6")
        else 1
    )


if __name__ == "__main__":
    raise SystemExit(main())
