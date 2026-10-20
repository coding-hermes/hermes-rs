#!/usr/bin/env python3
"""Validate Hermes RS project contract artifacts before implementation begins."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BOARD = ROOT / ".coding-hermes/board/tasks.jsonl"
REQUIRED_TASKS = {"HRS-001", "HRS-002", "HRS-003", "HRS-004"}

# Documented column list vs DDL (DF-HRS-115). The per-model usage rollup is
# write-only through the shim boundary — no ReadTxn/adapter method returns those
# rows — so consumers read `session_model_usage` with SQL and must know its
# columns. The lists below are prose that can drift from the DDL in
# src/persistence/upstream_ddl.rs, so both surfaces are parsed and compared
# here.
#
# Parsing convention (both surfaces must follow it): the documented list is the
# run of `backticked` names between the anchor phrase and the next parenthetical
# that cites the DDL or the row id.
USAGE_TABLE = "session_model_usage"
DDL_REL = Path("src/persistence/upstream_ddl.rs")
USAGE_DOC_SURFACES = (
    Path("specs/persistence-shim.md"),
    Path("skills/hermes-rs-usage/SKILL.md"),
)
COLUMN_ANCHOR = re.compile(r"its\s+columns,\s+in\s+the\s+DDL's\s+order,\s+are\s*")
ANCHOR_HINT = "its columns, in the DDL's order, are"
COLUMN_TERMINATOR = re.compile(
    r"\([^()]*(?:src/persistence/upstream_ddl\.rs|DF-HRS-\d+)[^()]*\)"
)
COLUMN_TOKEN = re.compile(r"`([A-Za-z_][A-Za-z0-9_]*)`")


def ddl_columns(root: Path, table: str) -> list[str]:
    """Column names of ``table`` in DDL order, from the shim's own DDL."""
    text = (root / DDL_REL).read_text(encoding="utf-8")
    marker = f"CREATE TABLE IF NOT EXISTS {table} ("
    start = text.find(marker)
    if start < 0:
        raise SystemExit(f"{DDL_REL}: no DDL for table {table!r}")
    body = text[start + len(marker) :].split("\n);", 1)[0]
    columns: list[str] = []
    for raw in body.splitlines():
        line = raw.strip()
        if not line or line.startswith("--") or line.upper().startswith("PRIMARY KEY"):
            continue
        columns.append(line.split()[0].rstrip(","))
    if not columns:
        raise SystemExit(f"{DDL_REL}: parsed no columns for table {table!r}")
    return columns


def documented_columns(text: str) -> list[str] | None:
    """The backticked column run following the documented anchor phrase."""
    anchor = COLUMN_ANCHOR.search(text)
    if anchor is None:
        return None
    terminator = COLUMN_TERMINATOR.search(text, anchor.end())
    if terminator is None:
        return None
    return COLUMN_TOKEN.findall(text[anchor.end() : terminator.start()])


def check_usage_column_docs(root: Path) -> list[str]:
    """Every documented usage column list must equal the DDL, in order."""
    errors: list[str] = []
    expected = ddl_columns(root, USAGE_TABLE)
    for rel in USAGE_DOC_SURFACES:
        path = root / rel
        if not path.exists():
            errors.append(f"missing documented surface: {rel}")
            continue
        documented = documented_columns(path.read_text(encoding="utf-8"))
        if documented is None:
            errors.append(
                f"{rel}: no documented {USAGE_TABLE} column list — expected the "
                f"phrase {ANCHOR_HINT!r} followed by a backticked list "
                "and a parenthetical citing the DDL"
            )
            continue
        missing = [name for name in expected if name not in documented]
        extra = [name for name in documented if name not in expected]
        if missing or extra:
            errors.append(
                f"{rel}: {USAGE_TABLE} column list disagrees with {DDL_REL} — "
                f"missing: {', '.join(missing) or 'none'}; "
                f"extra: {', '.join(extra) or 'none'}"
            )
        elif documented != expected:
            errors.append(
                f"{rel}: {USAGE_TABLE} column list has the right names in the "
                f"wrong order — documented: {', '.join(documented)}; "
                f"DDL: {', '.join(expected)}"
            )
    return errors


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

    errors = check_usage_column_docs(ROOT)
    if errors:
        print("FAIL: documented usage columns disagree with the DDL:")
        for error in errors:
            print(f"  - {error}")
        return 1

    print(
        f"{USAGE_TABLE} column list matches the DDL "
        f"({len(ddl_columns(ROOT, USAGE_TABLE))} columns, "
        f"{len(USAGE_DOC_SURFACES)} documented surfaces)"
    )
    print(
        f"project artifacts valid: {len(tasks)} board rows, {len(REQUIRED_TASKS)} kickoff tasks"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
