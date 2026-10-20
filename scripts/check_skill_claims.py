#!/usr/bin/env python3
"""Guard the usage skill's board-row claims against the actual board.

Why: skills/hermes-rs-usage/SKILL.md teaches the next agent which defects are
live. At the start of dogfood run 3 (HEAD 7331204) its Traps section still
listed DF-HRS-104..111 as OPEN and taught a workaround for each, while all
eight rows were already `complete` on .coding-hermes/board/tasks.jsonl — the
prose had drifted from the board (finding DF-HRS-113). Nothing else keeps
prose claims about board rows in step, so this checker extracts every
`DF-HRS-<n>` citation (expanding `DF-HRS-a..b` ranges), resolves each cited
row's status on the board, and fails when a claim's staleness markers
contradict a `complete` row, or when a cited id does not exist at all.

Design (conservative on purpose):
- claim unit = the SENTENCE containing the citation (markdown is split on
  sentence-ending punctuation; wrapped lines stay inside their sentence);
- only unambiguous staleness markers count: the words "open", "still", "yet",
  "workaround(s)" (word-boundary, case-insensitive) and the phrases
  "do not trust", "until fixed";
- a sentence carrying closed-defect language ("used to", "are/is/were
  (now) closed", "no longer", "do not/don't repeat", "re-proved/re-verified")
  is treated as DESCRIBING history, not asserting staleness, and is exempt —
  e.g. the current Traps preamble "The eight defects this section used to
  list as OPEN (`DF-HRS-104..111`) are closed ... do NOT repeat the old
  workarounds." must NOT fire even though it cites eight complete rows;
- only rows the board marks complete/closed/done/fixed/resolved can make a
  claim stale; pending/unknown statuses never fail a marker;
- a cited id missing from the board fails regardless of claim text (a
  citation of a pruned/typo'd row is drift no matter how it is phrased).

Board reading is tolerant by design (heterogeneous rows: missing keys, extra
keys, non-array values): blank lines are skipped, a whole-file JSON array is
accepted, non-dict lines are scanned for row objects, and a missing or
oddly-typed status never crashes the check — it just cannot make a claim
stale. Unparseable lines are counted and reported on stderr.

Known conservative misses (accepted, documented on DF-HRS-113): the pre-run-3
fixture's DF-HRS-108/110/111 bullets teach stale workarounds ("rotate
destinations per run", "needs a `sqlite3` CLI", "never clean") in wording
that carries none of the unambiguous markers above; the guard does not guess
semantics and lets them pass. The negative control still fails on
DF-HRS-104/105/106/107, which is the proof the guard catches real drift.

Controls (recorded 2026-10-20, worktree wt/DF-HRS-113 @ base 4c56327):

  $ python3 scripts/check_skill_claims.py --file scripts/fixtures/skill_claims_prerun3.md ; echo "exit=$?"
  scripts/fixtures/skill_claims_prerun3.md:31: DF-HRS-104 is 'complete' on the board but the skill claims otherwise (marker '\bopen\b'): "Open rows: `DF-HRS-104`, `DF-HRS-105`."
  scripts/fixtures/skill_claims_prerun3.md:31: DF-HRS-105 is 'complete' on the board but the skill claims otherwise (marker '\bopen\b'): "Open rows: `DF-HRS-104`, `DF-HRS-105`."
  scripts/fixtures/skill_claims_prerun3.md:35: DF-HRS-106 is 'complete' on the board but the skill claims otherwise (marker '\byet\b'): "A fresh store reports `created_new_store: false` and `degraded: ["fresh-store census mismatch after scaffold"]` (`DF-HRS-106`, `DF-HRS-107`) — do not branch on that flag and do not treat the degraded line as a real fault yet."
  scripts/fixtures/skill_claims_prerun3.md:35: DF-HRS-107 is 'complete' on the board but the skill claims otherwise (marker '\byet\b'): "A fresh store reports `created_new_store: false` and `degraded: ["fresh-store census mismatch after scaffold"]` (`DF-HRS-106`, `DF-HRS-107`) — do not branch on that flag and do not treat the degraded line as a real fault yet."
  exit=1

  $ python3 scripts/check_skill_claims.py ; echo "exit=$?"
  exit=0

`--self-test` re-runs both controls (against the committed fixture and the
live SKILL.md) plus three synthetic probes (missing row, suppressed history
sentence, genuine stale claim) and exits non-zero if any expected outcome
flips.

Usage:
  python3 scripts/check_skill_claims.py               # terse: silent on pass
  python3 scripts/check_skill_claims.py --file PATH   # check another file
  python3 scripts/check_skill_claims.py --violations  # explicit listing
  python3 scripts/check_skill_claims.py --json        # machine-readable
  python3 scripts/check_skill_claims.py --self-test   # run the controls

Exit codes: 0 = every cited claim agrees with the board; 1 = violations
found (one line each on stdout); 2 = environment error (missing file, empty
board).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SKILL = ROOT / "skills" / "hermes-rs-usage" / "SKILL.md"
DEFAULT_BOARD = ROOT / ".coding-hermes" / "board" / "tasks.jsonl"
FIXTURE = ROOT / "scripts" / "fixtures" / "skill_claims_prerun3.md"

CITATION_RE = re.compile(r"DF-HRS-(\d+)")
RANGE_RE = re.compile(r"DF-HRS-(\d+)\.\.(\d+)")
SENTENCE_SPLIT_RE = re.compile(r"(?<=[.!?])\s+")

COMPLETE_STATUSES = {"complete", "closed", "done", "fixed", "resolved"}
MAX_RANGE_SPAN = 100  # a wider "range" is prose, not a citation list

MARKER_RES = [
    re.compile(pattern, re.IGNORECASE)
    for pattern in (
        r"\bopen\b",
        r"\bstill\b",
        r"\byet\b",
        r"\bworkarounds?\b",
        r"\bdo\s+not\s+trust\b",
        r"\buntil\s+fixed\b",
    )
]

CLOSED_CONTEXT_RES = [
    re.compile(pattern, re.IGNORECASE)
    for pattern in (
        r"\bused\s+to\b",
        r"\b(?:are|is|were)\s+(?:now\s+)?closed\b",
        r"\bno\s+longer\b",
        r"\bdo\s+not\s+repeat\b",
        r"\bdon'?t\s+repeat\b",
        r"\bre-?(?:proved|verified)\b",
    )
]


def cited_ids(sentence: str) -> list[str]:
    """DF-HRS ids cited by a sentence, ranges expanded, order-stable."""
    ids: dict[str, None] = {}
    consumed: list[tuple[int, int]] = []
    for match in RANGE_RE.finditer(sentence):
        lo, hi = int(match.group(1)), int(match.group(2))
        if lo <= hi and hi - lo <= MAX_RANGE_SPAN:
            for number in range(lo, hi + 1):
                ids["DF-HRS-%d" % number] = None
        consumed.append(match.span())
    for match in CITATION_RE.finditer(sentence):
        if any(start <= match.start() < end for start, end in consumed):
            continue
        ids["DF-HRS-%s" % match.group(1)] = None
    return list(ids)


def iter_sentences(text: str):
    start = 0
    for match in SENTENCE_SPLIT_RE.finditer(text):
        piece = text[start : match.start()]
        if piece.strip():
            yield start, piece
        start = match.end()
    tail = text[start:]
    if tail.strip():
        yield start, tail


def _row_candidates(value: object):
    if isinstance(value, dict):
        yield value
    elif isinstance(value, list):
        for item in value:
            if isinstance(item, dict):
                yield item


def _status_text(row: dict) -> str:
    for key in ("status", "state"):
        value = row.get(key)
        if isinstance(value, str):
            text = value.strip().lower()
            if text:
                return text
        elif isinstance(value, list):
            for item in value:
                if isinstance(item, str) and item.strip():
                    return item.strip().lower()
        elif value is not None:
            text = str(value).strip().lower()
            if text:
                return text
    return ""


def load_board(path: Path) -> tuple[dict[str, str], int]:
    """Return {row id: status text} plus the count of unparseable lines."""
    text = path.read_text(encoding="utf-8")
    statuses: dict[str, str] = {}
    parse_errors = 0
    rows: list[object] = []
    try:
        whole = json.loads(text)
    except json.JSONDecodeError:
        whole = None
    if isinstance(whole, list):
        rows = whole
    else:
        for raw in text.splitlines():
            line = raw.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                parse_errors += 1
    for value in rows:
        for row in _row_candidates(value):
            row_id = row.get("id")
            if isinstance(row_id, str) and row_id.strip():
                statuses[row_id.strip()] = _status_text(row)
    return statuses, parse_errors


def check_skill(skill_text: str, board: dict[str, str], display: str):
    """Return (violations, citations_checked, suppressed_citations)."""
    violations: list[dict[str, object]] = []
    checked = 0
    suppressed = 0
    for start, sentence in iter_sentences(skill_text):
        ids = cited_ids(sentence)
        if not ids:
            continue
        line_no = skill_text.count("\n", 0, start) + 1
        claim = " ".join(sentence.split())
        if len(claim) > 240:
            claim = claim[:237] + "..."
        exempt = any(rx.search(sentence) for rx in CLOSED_CONTEXT_RES)
        marker = next((rx for rx in MARKER_RES if rx.search(sentence)), None)
        for row_id in ids:
            checked += 1
            status = board.get(row_id)
            if status is None:
                violations.append(
                    {
                        "kind": "missing_row",
                        "file": display,
                        "line": line_no,
                        "row_id": row_id,
                        "claim": claim,
                    }
                )
                continue
            if exempt:
                suppressed += 1
                continue
            if status not in COMPLETE_STATUSES or marker is None:
                continue
            violations.append(
                {
                    "kind": "stale_claim",
                    "file": display,
                    "line": line_no,
                    "row_id": row_id,
                    "status": status,
                    "marker": marker.pattern,
                    "claim": claim,
                }
            )
    return violations, checked, suppressed


def render(violation: dict[str, object], board_display: str) -> str:
    if violation["kind"] == "missing_row":
        return (
            '%s:%d: %s is cited in the skill but not present on the board (%s): "%s"'
            % (
                violation["file"],
                violation["line"],
                violation["row_id"],
                board_display,
                violation["claim"],
            )
        )
    return (
        "%s:%d: %s is '%s' on the board but the skill claims otherwise (marker %s): \"%s\""
        % (
            violation["file"],
            violation["line"],
            violation["row_id"],
            violation["status"],
            violation["marker"],
            violation["claim"],
        )
    )


def _read_or_die(path: Path, what: str) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise SystemExit("error: cannot read %s %s: %s" % (what, path, exc))


def _resolve(path: Path) -> Path:
    if path.exists() or path.is_absolute():
        return path
    candidate = ROOT / path
    return candidate if candidate.exists() else path


def run(args: argparse.Namespace) -> int:
    skill_path = _resolve(args.file) if args.file else DEFAULT_SKILL
    board_path = _resolve(args.board) if args.board else DEFAULT_BOARD
    skill_text = _read_or_die(skill_path, "skill file")
    board, parse_errors = load_board(board_path)
    if not board:
        raise SystemExit(
            "error: no board rows parsed from %s; refusing to judge claims" % board_path
        )
    if parse_errors:
        print(
            "note: %d unparseable line(s) skipped in %s" % (parse_errors, board_path),
            file=sys.stderr,
        )
    violations, checked, suppressed = check_skill(skill_text, board, str(skill_path))
    if args.json:
        print(
            json.dumps(
                {
                    "skill": str(skill_path),
                    "board": str(board_path),
                    "citations_checked": checked,
                    "suppressed_history": suppressed,
                    "violations": violations,
                },
                indent=2,
            )
        )
    elif violations:
        for violation in violations:
            print(render(violation, str(board_path)))
    if violations:
        return 1
    return 0


def self_test() -> int:
    failures: list[str] = []
    board, _ = load_board(DEFAULT_BOARD)
    if not board:
        print("self-test FAIL: board at %s is empty" % DEFAULT_BOARD, file=sys.stderr)
        return 1

    # 1. Negative control: the pre-run-3 fixture must FAIL and name the rows.
    fixture_violations, _, _ = check_skill(
        _read_or_die(FIXTURE, "fixture"), board, str(FIXTURE)
    )
    fixture_rows = sorted({str(v["row_id"]) for v in fixture_violations})
    expected_rows = [
        "DF-HRS-104",
        "DF-HRS-105",
        "DF-HRS-106",
        "DF-HRS-107",
    ]
    if fixture_rows != expected_rows:
        failures.append(
            "negative control: expected violations for %s, got %s"
            % (expected_rows, fixture_rows)
        )

    # 2. Positive control: the current SKILL.md must PASS.
    current_violations, _, _ = check_skill(
        _read_or_die(DEFAULT_SKILL, "skill"), board, str(DEFAULT_SKILL)
    )
    if current_violations:
        failures.append(
            "positive control: current SKILL.md must pass, got: %s"
            % [render(v, str(DEFAULT_BOARD)) for v in current_violations]
        )

    # 3. A cited id absent from the board must fail.
    missing_violations, _, _ = check_skill(
        "See `DF-HRS-999` for details.", {"DF-HRS-104": "complete"}, "synthetic"
    )
    if not any(
        v["kind"] == "missing_row" and v["row_id"] == "DF-HRS-999"
        for v in missing_violations
    ):
        failures.append("missing-row probe: DF-HRS-999 must be flagged")

    # 4. A sentence describing a closed defect must NOT fire (no false
    #    positive on legitimate history, per the DF-HRS-113 brief).
    history_violations, _, _ = check_skill(
        "The defects this section used to list as OPEN (`DF-HRS-104`) are"
        " closed — do NOT repeat the old workarounds.",
        {"DF-HRS-104": "complete"},
        "synthetic",
    )
    if history_violations:
        failures.append(
            "history-sentence probe must be suppressed, got: %s" % history_violations
        )

    # 5. A genuine stale claim against a complete row must fire.
    stale_violations, _, _ = check_skill(
        "`backup` still refuses an existing destination (`DF-HRS-104`).",
        {"DF-HRS-104": "complete"},
        "synthetic",
    )
    if not any(v["kind"] == "stale_claim" for v in stale_violations):
        failures.append("stale-claim probe: 'still refuses' must be flagged")

    if failures:
        for failure in failures:
            print("self-test FAIL: %s" % failure, file=sys.stderr)
        return 1
    print(
        "self-test: 5/5 probes pass (fixture fails on %s; current content"
        " passes; missing-row flagged; history sentence suppressed; stale"
        " claim flagged)" % ",".join(expected_rows)
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Check the usage skill's board-row claims against the board."
    )
    parser.add_argument(
        "--file",
        type=Path,
        default=None,
        help="skill file to check (default: skills/hermes-rs-usage/SKILL.md)",
    )
    parser.add_argument(
        "--board",
        type=Path,
        default=None,
        help="board JSONL to resolve statuses against (default: "
        ".coding-hermes/board/tasks.jsonl)",
    )
    parser.add_argument(
        "--violations",
        action="store_true",
        help="list violations explicitly (this is already the default output "
        "when the check fails)",
    )
    parser.add_argument(
        "--json", action="store_true", help="emit a JSON report instead of lines"
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the recorded negative/positive controls plus synthetic probes",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
