#!/usr/bin/env python3
"""Build a structured upstream-release snapshot for hermes-rs (HRS-004).

Fetches release metadata and a tag-to-tag compare from the GitHub API for
``NousResearch/hermes-agent`` and writes a deterministic JSON snapshot to
``docs/upstream-releases/<tag>.json``:

- sorted keys, no volatile fields (no fetch timestamps), stable file ordering
  (changed files sorted by path, area buckets sorted deterministically);
- release-notes body kept verbatim (exactly as the API returned it);
- changed-file summary against the PREVIOUS tag when one resolves, else
  against the HRS-001 compatibility pin (specs/COMPATIBILITY_MATRIX.md);
- pointers (paths + URLs, never file copies) into the source/config/tests/
  migrations areas;
- an honest ``compare.file_list_complete`` flag: the GitHub compare endpoint
  caps its ``files`` array at 300 entries (documented API limit), so a
  comparison at the cap is recorded as incomplete, never silently truncated.

Standard library only. Python >= 3.11. Re-running for the same tag is
idempotent: identical snapshot bytes (compare-and-replace, so re-runs do not
touch the file mtime either).

Usage:
    scripts/track_upstream_release.sh                # resolve latest release
    scripts/track_upstream_release.sh v0.22.2        # explicit tag
    scripts/track_upstream_release.sh v0.22.2 --prev v0.22.1
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from release_areas import (  # noqa: E402
    PIN_COMMIT,
    PIN_TAG,
    UPSTREAM_REPO,
    bucket_for,
    churn,
    classify_path,
    release_version,
    top_paths,
)

API_BASE = f"https://api.github.com/repos/{UPSTREAM_REPO}"
DEFAULT_OUTDIR = REPO_ROOT / "docs" / "upstream-releases"
# Documented GitHub compare-API limit for the `files` array.
COMPARE_FILES_CAP = 300
USER_AGENT = "hermes-rs-release-tracking (HRS-004)"


class UpstreamError(RuntimeError):
    """Raised when upstream data cannot be fetched or is inconsistent."""


def _request_json(url: str) -> object:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            if resp.status != 200:
                raise UpstreamError(f"GET {url}: HTTP {resp.status}")
            return json.load(resp)
    except UpstreamError:
        raise
    except Exception as exc:  # noqa: BLE001 - report one clear failure line
        raise UpstreamError(f"GET {url}: {exc}") from exc


def resolve_latest_release() -> dict[str, object]:
    data = _request_json(f"{API_BASE}/releases/latest")
    if not isinstance(data, dict) or not data.get("tag_name"):
        raise UpstreamError("releases/latest returned no tag_name")
    return data


def fetch_release(tag: str) -> dict[str, object]:
    data = _request_json(f"{API_BASE}/releases/tags/{tag}")
    if not isinstance(data, dict) or data.get("tag_name") != tag:
        raise UpstreamError(f"release lookup for {tag} returned unexpected data")
    return data


def list_release_tags() -> list[str]:
    """All release tag names, newest first (releases API, non-draft)."""
    tags: list[str] = []
    page = 1
    while True:
        data = _request_json(f"{API_BASE}/releases?per_page=100&page={page}")
        if not isinstance(data, list) or not data:
            break
        for item in data:
            if isinstance(item, dict) and not item.get("draft"):
                tag = item.get("tag_name")
                if isinstance(tag, str):
                    tags.append(tag)
        if len(data) < 100:
            break
        page += 1
    return tags


def previous_tag_for(tag: str) -> str | None:
    """The newest release tag that sorts strictly before ``tag``."""
    try:
        tags = [t for t in list_release_tags() if t != tag]
    except UpstreamError:
        return None
    older = [t for t in tags if release_version(t) < release_version(tag)]
    if not older:
        return None
    return max(older, key=release_version)


def build_compare(base: str, head: str) -> dict[str, object]:
    data = _request_json(f"{API_BASE}/compare/{base}...{head}")
    if not isinstance(data, dict) or not isinstance(data.get("files"), list):
        raise UpstreamError(f"compare {base}...{head}: malformed response")
    return data


def normalize_files(raw_files: list[object]) -> list[dict[str, object]]:
    files: list[dict[str, object]] = []
    for item in raw_files:
        if not isinstance(item, dict):
            continue
        path = item.get("filename")
        if not isinstance(path, str):
            continue
        files.append(
            {
                "path": path,
                "status": str(item.get("status", "unknown")),
                "additions": int(item.get("additions", 0) or 0),
                "deletions": int(item.get("deletions", 0) or 0),
            }
        )
    files.sort(key=lambda f: str(f["path"]))
    return files


def summarize_files(files: list[dict[str, object]]) -> dict[str, object]:
    areas: dict[str, dict[str, int]] = {}
    buckets: dict[str, dict[str, int]] = {}
    for entry in files:
        area = classify_path(str(entry["path"]))
        bucket = bucket_for(str(entry["path"]))
        stats = areas.setdefault(area, {"files": 0, "additions": 0, "deletions": 0})
        stats["files"] += 1
        stats["additions"] += int(entry["additions"])
        stats["deletions"] += int(entry["deletions"])
        bstats = buckets.setdefault(
            bucket, {"files": 0, "additions": 0, "deletions": 0, "churn": 0}
        )
        bstats["files"] += 1
        bstats["additions"] += int(entry["additions"])
        bstats["deletions"] += int(entry["deletions"])
        bstats["churn"] += churn(entry)
    per_area_paths: dict[str, list[tuple[int, str]]] = {}
    for entry in files:
        area = classify_path(str(entry["path"]))
        per_area_paths.setdefault(area, []).append((churn(entry), str(entry["path"])))
    top_by_area: dict[str, list[str]] = {}
    for area, ranked in per_area_paths.items():
        ranked.sort(key=lambda t: (-t[0], t[1]))
        top_by_area[area] = [p for _, p in ranked[:5]]
    return {
        "total_files": len(files),
        "total_additions": sum(int(f["additions"]) for f in files),
        "total_deletions": sum(int(f["deletions"]) for f in files),
        "areas": {k: areas[k] for k in sorted(areas)},
        "top_paths_by_area": {k: top_by_area[k] for k in sorted(top_by_area)},
        "top_buckets": {
            k: buckets[k]
            for k in sorted(buckets, key=lambda b: (-buckets[b]["churn"], b))
        },
    }


def area_pointers(
    files: list[dict[str, object]], compare_url: str
) -> dict[str, object]:
    """Pointers (paths + URLs) into the source/config/tests/migrations areas."""
    pointers: dict[str, list[str]] = {}
    wanted = ("config", "migrations", "source", "tests")
    for entry in files:
        area = classify_path(str(entry["path"]))
        if area in wanted:
            pointers.setdefault(area, []).append(str(entry["path"]))
    result: dict[str, object] = {}
    for area in wanted:
        paths = sorted(pointers.get(area, []))
        if not paths:
            result[area] = {
                "count": 0,
                "paths": [],
                "note": f"no {area}-area files in this comparison",
            }
            continue
        shown = paths[:20]
        result[area] = {
            "count": len(paths),
            "paths": shown,
            "urls": [
                f"https://github.com/{UPSTREAM_REPO}/blob/main/{p}" for p in shown
            ],
            "note": (
                "pointers only — first 20 paths, sorted; full set is derivable "
                "from the recorded compare_url"
                if len(paths) > len(shown)
                else "pointers only — full set for this comparison"
            ),
        }
    result["compare_url"] = compare_url
    return result


def build_snapshot(
    release: dict[str, object],
    compare: dict[str, object],
    base_ref: str,
    base_mode: str,
) -> dict[str, object]:
    tag = str(release.get("tag_name", ""))
    files = normalize_files(list(compare.get("files", [])))
    file_list_complete = bool(files) and len(files) < COMPARE_FILES_CAP
    compare_url = f"https://github.com/{UPSTREAM_REPO}/compare/{base_ref}...{tag}"
    base_commit = compare.get("base_commit")
    base_sha = (
        str(base_commit.get("sha", "unknown"))
        if isinstance(base_commit, dict)
        else "unknown"
    )
    head_sha = str(release.get("target_commitish") or "unknown")

    snapshot: dict[str, object] = {
        "schema": "hermes-rs/upstream-release-snapshot",
        "schema_version": 1,
        "tag": tag,
        "release_name": release.get("name"),
        "published_at": release.get("published_at"),
        "created_at": release.get("created_at"),
        "prerelease": bool(release.get("prerelease", False)),
        "draft": bool(release.get("draft", False)),
        "release_url": release.get("html_url"),
        "head_sha": head_sha,
        "release_notes": {
            "body": release.get("body"),
            "source": f"{API_BASE}/releases/tags/{tag}",
        },
        "compare": {
            "base": {"ref": base_ref, "sha": base_sha, "mode": base_mode},
            "head": {"ref": tag, "sha": head_sha},
            "url": compare_url,
            "api_url": f"{API_BASE}/compare/{base_ref}...{tag}",
            "total_commits": int(compare.get("total_commits", 0) or 0),
            "ahead_by": int(compare.get("ahead_by", 0) or 0),
            "file_list_complete": file_list_complete,
            "file_list_cap": COMPARE_FILES_CAP,
            "summary": summarize_files(files),
            "changed_files_top3": [
                {"path": p, "area": classify_path(p)} for p in top_paths(files, limit=3)
            ],
        },
        "area_pointers": area_pointers(files, compare_url),
        "tool": {
            "script": "scripts/track_upstream_release.sh",
            "classification": "scripts/release_areas.py",
            "github_api_repo": UPSTREAM_REPO,
        },
    }
    return snapshot


def write_idempotent(path: Path, payload: bytes) -> str:
    """Write only when bytes differ; returns 'written' or 'unchanged'."""
    if path.exists() and path.read_bytes() == payload:
        return "unchanged"
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_bytes(payload)
    os.replace(tmp, path)
    return "written"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "tag", nargs="?", default=None, help="upstream tag (default: latest release)"
    )
    parser.add_argument(
        "--prev", default=None, help="explicit previous tag for the compare"
    )
    parser.add_argument(
        "--outdir",
        default=str(DEFAULT_OUTDIR),
        help=f"output directory (default: {DEFAULT_OUTDIR})",
    )
    args = parser.parse_args(argv)

    try:
        release = fetch_release(args.tag) if args.tag else resolve_latest_release()
        tag = str(release["tag_name"])

        prev = args.prev or previous_tag_for(tag)
        if prev:
            compare = build_compare(prev, tag)
            base_ref, base_mode = prev, "previous-tag"
        else:
            compare = build_compare(PIN_TAG, tag)
            base_ref, base_mode = (
                PIN_TAG,
                "compatibility-pin (no previous release tag resolved)",
            )

        base_sha = str((compare.get("base_commit") or {}).get("sha", "unknown"))
        if base_mode == "compatibility-pin" and base_sha != PIN_COMMIT:
            print(
                f"WARN: pin {PIN_TAG} expected commit {PIN_COMMIT} but compare "
                f"base_commit is {base_sha}; snapshot records the compare value",
                file=sys.stderr,
            )

        snapshot = build_snapshot(release, compare, base_ref, base_mode)

        outdir = Path(args.outdir)
        outdir.mkdir(parents=True, exist_ok=True)
        out = outdir / f"{tag}.json"
        payload = (json.dumps(snapshot, indent=2, sort_keys=True) + "\n").encode(
            "utf-8"
        )
        status = write_idempotent(out, payload)
        summary = snapshot["compare"]
        assert isinstance(summary, dict)
        print(f"{status}: {out} ({len(payload)} bytes)")
        print(
            f"tag={tag} compare_base={base_ref} files={summary['summary']['total_files']} "
            f"complete={summary['file_list_complete']}"
        )
        return 0
    except UpstreamError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        print(
            "No snapshot was written. Re-run when network access is available; "
            "do not fabricate snapshot content.",
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
