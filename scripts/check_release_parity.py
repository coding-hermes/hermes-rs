#!/usr/bin/env python3
"""Release-parity gate for hermes-rs (HRS-004).

Reads a release snapshot (``docs/upstream-releases/<tag>.json``, as written
by ``scripts/track_upstream_release.sh``) and regenerates
``docs/upstream-releases/PARITY.md`` deterministically:

- every NON-trivial observed upstream change area becomes a parity item
  (``PARI-<n>``) appended to ``docs/upstream-releases/parity-ledger.md``
  (JSON-lines content — one JSON object per line — despite the .md
  extension, which the task contract names);
- areas with a standing rationale (docs/locales/ci/contributors) are recorded
  as explicit no-action rows with that rationale — never as parity claims;
- deterministic output: re-running on the same inputs regenerates
  byte-identical PARITY.md and performs no ledger appends (already-present
  ``<tag>/<area>`` pairs are skipped). Only genuinely new pairs append.

This gate gates on EVIDENCE PRESENCE, not on claims: a parity item exists to
be *triaged* — until a triage status is recorded, the report says
"pending triage" and asserts nothing about compatibility. No drop-in,
compatibility, or resource claim is emitted anywhere.

Usage:
    scripts/check_release_parity.py                       # newest snapshot
    scripts/check_release_parity.py docs/upstream-releases/v0.22.2.json
    scripts/check_release_parity.py --snapshot tests/fixtures/sample-snapshot.json \
        --ledger /tmp/ledger.jsonl --parity /tmp/PARITY.md
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from release_areas import (  # noqa: E402
    NO_ACTION_RATIONALES,
    PIN_COMMIT,
    PIN_TAG,
    related_surfaces,
)

DEFAULT_DOCDIR = REPO_ROOT / "docs" / "upstream-releases"
LEDGER_PATH = DEFAULT_DOCDIR / "parity-ledger.md"
PARITY_PATH = DEFAULT_DOCDIR / "PARITY.md"

# Areas that must exist in a snapshot summary for the report to be complete.
EXPECTED_AREAS = ("config", "migrations", "source", "tests")

# Standing no-action areas (same set as release_areas.NO_ACTION_RATIONALES
# — kept inline as documentation of the gate contract).
NO_ACTION_AREAS = ("ci", "contributors", "docs", "locales")

# Parity-item areas: any observed non-trivial area not in NO_ACTION_AREAS.
# Order defines PARI numbering order (alphabetical, stable). Together the
# two sets cover every area the classifier can emit — the gate leaves no
# observed area unassigned: item or rationale, never silence.
PARITY_ITEM_AREAS = ("config", "migrations", "other", "packaging", "source", "tests")


def load_snapshot(path: Path) -> dict[str, object]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if (
        not isinstance(data, dict)
        or data.get("schema") != "hermes-rs/upstream-release-snapshot"
    ):
        raise SystemExit(f"{path}: not a hermes-rs upstream-release snapshot")
    for key in ("tag", "published_at", "compare"):
        if key not in data:
            raise SystemExit(f"{path}: snapshot missing required key '{key}'")
    return data


def read_existing_pairs(ledger: Path) -> dict[tuple[str, str], str]:
    """Already-ledgered (tag, area) pairs -> their PARI ids. Corrupt lines
    are loud, not silent."""
    if not ledger.exists():
        return {}
    pairs: dict[tuple[str, str], str] = {}
    for lineno, raw in enumerate(
        ledger.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if not raw.strip():
            continue
        try:
            row = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"{ledger}:{lineno}: invalid JSON: {exc}") from exc
        pairs[(str(row.get("tag")), str(row.get("area")))] = str(row.get("id", ""))
    return pairs


def append_ledger_rows(
    ledger: Path,
    snapshot: dict[str, object],
    areas: list[str],
    start_id: int,
    snapshot_ref: str,
) -> dict[str, str]:
    """Append one ledger row per new (tag, area); returns area -> PARI id."""
    tag = str(snapshot["tag"])
    existing = read_existing_pairs(ledger)
    assigned: dict[str, str] = {}
    rows: list[dict[str, object]] = []
    next_id = start_id
    for area in areas:
        if (tag, area) in existing:
            continue
        pid = f"PARI-{next_id:03d}"
        assigned[area] = pid
        rows.append(
            {
                "id": pid,
                "tag": tag,
                "area": area,
                "status": "pending triage",
                "source": "scripts/check_release_parity.py",
                "snapshot": snapshot_ref,
                "task": "HRS-004",
                "action": f"triage changed {area} files into parity work or a reviewed no-action exclusion",
            }
        )
        next_id += 1
    if rows:
        with ledger.open("a", encoding="utf-8") as fh:
            for row in rows:
                fh.write(json.dumps(row, sort_keys=True) + "\n")
    return assigned


def render_parity(
    snapshot: dict[str, object],
    assigned: dict[str, str],
    no_action: list[dict[str, str]],
    parity_path: Path,
    snapshot_path: Path,
    ledger_path: Path,
) -> str:
    tag = str(snapshot["tag"])
    compare = snapshot["compare"]
    assert isinstance(compare, dict)
    summary = compare.get("summary", {})
    assert isinstance(summary, dict)
    areas = summary.get("areas", {})
    assert isinstance(areas, dict)
    top_by_area = summary.get("top_paths_by_area", {})
    assert isinstance(top_by_area, dict)
    base = compare.get("base", {})
    assert isinstance(base, dict)

    lines: list[str] = []
    lines.append(f"# Upstream release parity report — {tag}")
    lines.append("")
    lines.append("Generated by `scripts/check_release_parity.py` from the committed")
    lines.append(
        f"snapshot [`{snapshot_path.name}`]({snapshot_path.name}). Deterministic:"
    )
    lines.append(
        "re-running the gate on the same inputs reproduces this file byte-for-byte."
    )
    lines.append("Items are triage queues, not parity claims: nothing here asserts")
    lines.append(
        "compatibility. Evidence-first gating per `specs/HERMES_RS_COMPATIBILITY.md`"
    )
    lines.append("(AC-5, AC-7): gaps stay visible until fixtures close them.")
    lines.append("")
    lines.append("## Release")
    lines.append("")
    lines.append("| Field | Value |")
    lines.append("|---|---|")
    lines.append(f"| Tag | `{tag}` |")
    lines.append(f"| Release | {snapshot.get('release_name', 'unknown')} |")
    lines.append(f"| Published | {snapshot.get('published_at', 'unknown')} |")
    release_url = snapshot.get("release_url")
    lines.append(f"| Release URL | {release_url} |")
    lines.append(
        f"| Upstream commit (head) | `{snapshot.get('head_sha', 'unknown')}` |"
    )
    base_mode = str(base.get("mode", "unknown"))
    lines.append(
        f"| Diff base | `{base.get('ref', 'unknown')}` @ `{base.get('sha', 'unknown')}` ({base_mode}) |"
    )
    lines.append(f"| Compare URL | {compare.get('url', 'unknown')} |")
    lines.append(
        f"| Commits in comparison | {compare.get('total_commits', 'unknown')} |"
    )
    if not compare.get("file_list_complete", False):
        cap = compare.get("file_list_cap", 300)
        lines.append(
            f"| Changed-file list | **incomplete** — at the GitHub compare-API "
            f"cap of {cap} files; counts are a floor, not the full diff |"
        )
    else:
        lines.append("| Changed-file list | complete |")
    lines.append("")

    lines.append("## Changed-area summary")
    lines.append("")
    lines.append("| Area | Files | +/− | Standing disposition |")
    lines.append("|---|---|---|---|")
    area_disp: dict[str, str] = {}
    for a in assigned:
        area_disp[a] = f"parity item [`{assigned[a]}`](#{assigned[a].lower()})"
    for row in no_action:
        area_disp[row["area"]] = "no action (standing rationale)"
    for area in sorted(areas):
        st = areas[area]
        assert isinstance(st, dict)
        counts = f"+{st.get('additions', 0)} / −{st.get('deletions', 0)}"
        lines.append(
            f"| {area} | {st.get('files', 0)} | {counts} | {area_disp.get(area, 'unassigned')} |"
        )
    lines.append("")

    lines.append(f"## Parity items ({len(assigned)})")
    lines.append("")
    if not assigned:
        lines.append("_No parity items for this release: every observed area carries a")
        lines.append(
            "standing no-action rationale. (This is a summary of the gate run,"
        )
        lines.append("not a compatibility claim.)_")
        lines.append("")
    else:
        lines.append(
            "Each item is a traceable triage queue entry (ledger row). Pending"
        )
        lines.append("triage = no parity claim permitted (matrix rule).")
        lines.append("")
    for area in sorted(assigned):
        pid = assigned[area]
        st = areas.get(area, {})
        assert isinstance(st, dict)
        top = top_by_area.get(area, [])
        surface_hints = related_surfaces(top)
        lines.append(f"### {pid} — `{area}` area ({tag})")
        lines.append("")
        lines.append(f"- **Status:** pending triage (ledger row `{pid}`)")
        lines.append(
            f"- **Observed:** {st.get('files', 0)} changed file(s), "
            f"+{st.get('additions', 0)} / −{st.get('deletions', 0)}"
        )
        if top:
            lines.append(
                f"- **Most-changed paths:** {', '.join(f'`{p}`' for p in top)}"
            )
        if surface_hints:
            lines.append(
                "- **Related matrix surfaces (hinted by changed paths; triage must confirm):** "
                + "; ".join(surface_hints)
            )
        lines.append(
            f"- **Required next step:** triage the `{area}`-area changes into parity "
            "work items or a reviewed no-action exclusion; update"
            " `specs/COMPATIBILITY_MATRIX.md` (matrix 'Update rule') before any status"
            " moves past *pending triage*."
        )
        lines.append("")

    lines.append(f"## No-action areas ({len(no_action)})")
    lines.append("")
    if no_action:
        lines.append(
            "Standing rationales from `scripts/release_areas.py` — recorded so the"
        )
        lines.append("absence of parity work is a decision, not an omission:")
        lines.append("")
        lines.append("| Area | Files | Rationale |")
        lines.append("|---|---|---|")
        for row in no_action:
            st = areas.get(row["area"], {})
            assert isinstance(st, dict)
            lines.append(
                f"| {row['area']} | {st.get('files', 0)} | {row['rationale']} |"
            )
        lines.append("")

    lines.append("## Known compatibility gaps (for the demonstrated release)")
    lines.append("")
    lines.append(
        f"Every parity item above is an OPEN gap for upstream `{tag}` relative to the"
        f" hermes-rs scope: no upstream change in a parity-item area has been triaged,"
        " let alone implemented, by hermes-rs. There is no compatibility claim to"
        " publish — the pin of record remains"
        f" `{PIN_TAG}` @ `{PIN_COMMIT}` (HRS-001) until the matrix is re-pinned by a"
        " later task. This section exists so the gate report, not a claim, is the"
        " release artifact."
    )
    lines.append("")
    lines.append("## Gate provenance")
    lines.append("")
    lines.append("| Field | Value |")
    lines.append("|---|---|")
    lines.append(f"| Snapshot | `{snapshot_path.name}` |")
    lines.append(f"| Ledger | `{ledger_path.name}` |")
    lines.append(f"| Parity items this run | {len(assigned)} |")
    lines.append(f"| No-action areas this run | {len(no_action)} |")
    lines.append("")
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "snapshot_pos",
        nargs="?",
        default=None,
        help="snapshot path (default: newest docs/upstream-releases/<tag>.json)",
    )
    parser.add_argument(
        "--snapshot", dest="snapshot_opt", default=None, help="snapshot path"
    )
    parser.add_argument("--ledger", default=None, help="ledger path override")
    parser.add_argument("--parity", default=None, help="PARITY.md output path override")
    args = parser.parse_args(argv)

    snapshot_arg = args.snapshot_opt or args.snapshot_pos
    docdir = DEFAULT_DOCDIR
    if snapshot_arg:
        snapshot_path = Path(snapshot_arg)
        docdir = snapshot_path.parent
    else:
        if not docdir.exists():
            raise SystemExit(f"no snapshots directory at {docdir}")
        candidates = [
            p
            for p in docdir.glob("*.json")
            if p.stem != "PARITY" and p.suffix == ".json"
        ]
        if not candidates:
            raise SystemExit(f"no release snapshots in {docdir}")
        snapshot_path = max(candidates, key=lambda p: release_version_key(p.stem))
    ledger_path = Path(args.ledger) if args.ledger else docdir / "parity-ledger.md"
    parity_path = Path(args.parity) if args.parity else docdir / "PARITY.md"

    snapshot = load_snapshot(snapshot_path)
    tag = str(snapshot["tag"])
    compare = snapshot["compare"]
    assert isinstance(compare, dict)
    summary = compare.get("summary", {})
    assert isinstance(summary, dict)
    areas = summary.get("areas", {})
    assert isinstance(areas, dict)

    pointers = snapshot.get("area_pointers")
    if isinstance(pointers, dict):
        missing = [a for a in EXPECTED_AREAS if a not in pointers]
        if missing:
            print(
                f"WARN: snapshot {snapshot_path.name} area_pointers lacks {missing}; "
                "snapshot may predate the pointer contract",
                file=sys.stderr,
            )
    else:
        print(
            f"WARN: snapshot {snapshot_path.name} has no area_pointers section",
            file=sys.stderr,
        )

    parity_areas = sorted(
        a for a in areas if a in PARITY_ITEM_AREAS and int(areas[a].get("files", 0)) > 0
    )
    no_action = [
        {"area": a, "rationale": NO_ACTION_RATIONALES[a]}
        for a in NO_ACTION_AREAS
        if a in areas and int(areas[a].get("files", 0)) > 0
    ]

    existing = read_existing_pairs(ledger_path)
    next_id = 1 + max(
        (
            int(pid.split("-")[1])
            for pid in existing.values()
            if pid.startswith("PARI-")
        ),
        default=0,
    )

    # Dry-run assignment for rendering; appends only for genuinely new pairs.
    new_areas = [a for a in parity_areas if (tag, a) not in existing]
    assigned_preview: dict[str, str] = {}
    probe = next_id
    for area in new_areas:
        assigned_preview[area] = f"PARI-{probe:03d}"
        probe += 1
    for area in parity_areas:
        if area not in assigned_preview:
            assigned_preview[area] = existing[(tag, area)]

    rendered = render_parity(
        snapshot, assigned_preview, no_action, parity_path, snapshot_path, ledger_path
    )
    new_bytes = rendered.encode("utf-8")
    if parity_path.exists() and parity_path.read_bytes() == new_bytes:
        print(f"unchanged: {parity_path}")
    else:
        parity_path.write_bytes(new_bytes)
        print(f"written: {parity_path} ({len(new_bytes)} bytes)")

    # Append AFTER rendering so ids in the report match appended rows exactly.
    try:
        snapshot_ref = str(snapshot_path.resolve().relative_to(REPO_ROOT))
    except ValueError:
        snapshot_ref = str(snapshot_path)
    assigned = append_ledger_rows(
        ledger_path, snapshot, parity_areas, next_id, snapshot_ref
    )
    if assigned:
        for area in sorted(assigned):
            print(f"ledgered: {assigned[area]} tag={tag} area={area}")
    else:
        print(
            f"ledger: no new rows for tag={tag} ({len(parity_areas)} parity areas already tracked)"
        )
    return 0


def release_version_key(stem: str) -> tuple[int, ...]:
    from release_areas import release_version

    return release_version(stem)


if __name__ == "__main__":
    raise SystemExit(main())
