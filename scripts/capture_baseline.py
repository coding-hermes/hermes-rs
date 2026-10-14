#!/usr/bin/env python3
"""Capture a resource baseline for a named workload profile (HRS-001).

Records environment metadata (uname, CPU count/model, python version, UTC
timestamp) plus the workload-profile fields with null/placeholder
measurements and a ``pending_implementation`` marker.

This script is a SCAFFOLD: no agent loop is executed and no measurement is
taken yet. The measurement pass (subprocess RSS/CPU sampling around a real
upstream run) lands with the implementation tasks; the profile definitions
and JSON shape here are the contract the capture pass must fill.

Standard library only. Python >= 3.11.

Usage:
    python3 scripts/capture_baseline.py --profile single_session --out /tmp/hrs001-baseline.json
    python3 scripts/capture_baseline.py --profile multi_session --duration 120 --out /tmp/hrs001-baseline.json
"""

from __future__ import annotations

import argparse
import json
import platform
import sys
from datetime import datetime, timezone
from pathlib import Path

PROFILES = {
    "single_session": {
        "description": (
            "Single-session interactive agent loop: one conversation, one "
            "scripted prompt per turn, tool calls enabled, streaming on."
        ),
        "workload": {
            "concurrency": 1,
            "sessions": 1,
            "tool_calls": "enabled (default core toolset)",
            "dataset": "pending measurement (fixed scripted prompt set)",
        },
    },
    "multi_session": {
        "description": (
            "Multi-session batch with tool calls: N concurrent sessions, "
            "each running a fixed scripted prompt set with tool calls, "
            "batch-style (no interactivity)."
        ),
        "workload": {
            "concurrency": "pending measurement (target 4)",
            "sessions": "pending measurement (target 8)",
            "tool_calls": "enabled (default core toolset)",
            "dataset": "pending measurement (fixed scripted prompt set)",
        },
    },
}

# Placeholder measurement fields the real capture pass must populate.
# All null here on purpose: no claim of performance is made by this scaffold.
MEASUREMENTS = {
    "duration_seconds": None,
    "cpu_percent_avg": None,
    "cpu_seconds_total": None,
    "rss_mb_peak": None,
    "rss_mb_end": None,
    "throughput_turns_per_minute": None,
    "latency_p50_ms": None,
    "latency_p95_ms": None,
    "latency_p99_ms": None,
    "storage_growth_mb": None,
}


def read_cpu_model() -> str:
    """Best-effort CPU model string (Linux /proc/cpuinfo, else platform)."""
    try:
        with open("/proc/cpuinfo", "r", encoding="utf-8") as fh:
            for line in fh:
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def read_os_release() -> str:
    """Best-effort distro string from /etc/os-release PRETTY_NAME."""
    try:
        with open("/etc/os-release", "r", encoding="utf-8") as fh:
            for line in fh:
                if line.startswith("PRETTY_NAME="):
                    return line.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    return sys.platform


def environment_metadata() -> dict:
    uname = platform.uname()
    return {
        "uname": " ".join(
            [uname.system, uname.node, uname.release, uname.version, uname.machine]
        ),
        "kernel": uname.release,
        "arch": uname.machine,
        "os_release": read_os_release(),
        "cpu_count": None,  # filled below (os.cpu_count may be None)
        "cpu_model": read_cpu_model(),
        "python_version": platform.python_version(),
        "python_implementation": platform.python_implementation(),
        "captured_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    }


def build_record(profile: str, duration: int) -> dict:
    env = environment_metadata()
    import os

    env["cpu_count"] = os.cpu_count()
    return {
        "schema": "hermes-rs/baseline/1",
        "profile": profile,
        "profile_description": PROFILES[profile]["description"],
        "workload": dict(PROFILES[profile]["workload"]),
        "requested_duration_seconds": duration,
        "pending_implementation": True,
        "environment": env,
        "measurements": dict(MEASUREMENTS),
        "notes": (
            "Scaffold record: measurements are null placeholders. The "
            "capture pass must run the pinned upstream reference and the "
            "Rust implementation under identical conditions and fill every "
            "field; nulls must carry a reason when they cannot be measured."
        ),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Capture a resource baseline record for a workload profile (HRS-001 scaffold)."
    )
    parser.add_argument(
        "--profile",
        choices=sorted(PROFILES),
        required=True,
        help="Workload profile to record.",
    )
    parser.add_argument(
        "--duration",
        type=int,
        default=60,
        help="Requested measurement duration in seconds (default: 60).",
    )
    parser.add_argument(
        "--out",
        required=True,
        help="Path to write the JSON baseline record to.",
    )
    args = parser.parse_args(argv)

    record = build_record(args.profile, args.duration)
    out_path = Path(args.out)
    if out_path.parent and str(out_path.parent):
        out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as fh:
        json.dump(record, fh, indent=2, sort_keys=True)
        fh.write("\n")
    print(
        f"baseline record written: {out_path} (profile={args.profile}, pending_implementation=true)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
