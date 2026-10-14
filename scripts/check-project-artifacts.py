#!/usr/bin/env python3
"""Validate Hermes RS project contract artifacts before implementation begins."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BOARD = ROOT / ".coding-hermes/board/tasks.jsonl"
REQUIRED_TASKS = {"HRS-001", "HRS-002", "HRS-003", "HRS-004"}


def main() -> int:
    tasks: list[dict[str, object]] = []
    for number, raw in enumerate(
        BOARD.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if not raw.strip():
            continue
        try:
            value = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"{BOARD}:{number}: invalid JSON: {exc}") from exc
        if not isinstance(value, dict):
            raise SystemExit(f"{BOARD}:{number}: expected a JSON object")
        tasks.append(value)

    ids = [row.get("id") for row in tasks]
    duplicates = sorted({str(item) for item in ids if ids.count(item) > 1})
    if duplicates:
        raise SystemExit(f"duplicate task ids: {', '.join(duplicates)}")
    missing = REQUIRED_TASKS - {str(item) for item in ids}
    if missing:
        raise SystemExit(f"missing kickoff tasks: {', '.join(sorted(missing))}")

    kickoff = next(row for row in tasks if row.get("id") == "HRS-001")
    criteria = kickoff.get("acceptance_criteria")
    if not isinstance(criteria, list) or not any(
        "same cases" in str(item).lower()
        and "python baseline" in str(item).lower()
        and "rust" in str(item).lower()
        for item in criteria
    ):
        raise SystemExit(
            "HRS-001 must enforce paired upstream Python/Rust harness scoring"
        )

    spec = (ROOT / "specs/HERMES_RS_COMPATIBILITY.md").read_text(encoding="utf-8")
    for required in ("AC-9:", "SQLite", "Turso", "PostgreSQL", "Python"):
        if required not in spec:
            raise SystemExit(
                f"compatibility spec missing required contract: {required}"
            )

    print(
        f"project artifacts valid: {len(tasks)} board rows, {len(REQUIRED_TASKS)} kickoff tasks"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
