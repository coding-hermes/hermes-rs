#!/usr/bin/env python3
"""HRS-007 paired-run driver.

Runs the vector corpus through the sealed adapter boundary against the
stub Rust target, emitting the versioned paired-run result format
(specs/TEST_ADAPTER_CONTRACT.md section 5).

Exit code: 0 iff every non-excluded vector PASSes. The default stub is
deliberately broken, so `python3 paired_run.py` exits 1 by design;
`python3 paired_run.py --broken false` exits 0.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import adapter  # noqa: E402

SCHEMA = "hermes-rs/paired-run/1"
REPO = HERE.parents[1]


def repo_sha() -> str:
    try:
        return (
            subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=str(REPO),
                capture_output=True,
                text=True,
                timeout=10,
            ).stdout.strip()
            or "unknown"
        )
    except Exception:
        return "unknown"


def file_sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--broken",
        default=None,
        choices=["true", "false"],
        help="override the stub target's broken flag",
    )
    ap.add_argument("--out", default=str(HERE / "last_paired_run.json"))
    ap.add_argument("--target-module", default="rust_target")
    args = ap.parse_args()

    broken = None if args.broken is None else (args.broken == "true")

    vectors, vector_ids = adapter.load_corpus()
    if len(vector_ids) != len(vectors) or len(set(vector_ids)) != len(vector_ids):
        print("FAIL: corpus ids not unique", file=sys.stderr)
        return 2

    manifest = json.loads(
        (REPO / "specs" / "TEST_CLASSIFICATION_MANIFEST.json").read_text(
            encoding="utf-8"
        )
    )

    results = []
    for vec in vectors:
        res = adapter.run_paired(vec, target_module=args.target_module, broken=broken)
        results.append(res)

    passed = sum(1 for r in results if r["status"] == "PASS")
    failed = sum(1 for r in results if r["status"] == "FAIL")
    skipped = sum(1 for r in results if r["status"] == "SKIP-EXCLUDED")
    total = len(results)
    ok = failed == 0 and passed + skipped == total

    doc = {
        "schema": SCHEMA,
        "denominator": {
            "total": total,
            "expected_total": len(vector_ids),
            "reconciles": total == len(vector_ids),
        },
        "counts": {
            "passed": passed,
            "failed": failed,
            "skip_excluded": skipped,
            "other": total - passed - failed - skipped,
        },
        "harness_sha": manifest["harness_sha"],
        "harness": {
            "upstream_repo": manifest["upstream"]["repo"],
            "upstream_tag": manifest["upstream"]["tag"],
            "runner": "scripts/run_tests.sh (per-file isolation) per specs/UPSTREAM_BASELINE.md",
        },
        "adapter": {
            "adapter_sha": file_sha(HERE / "adapter.py"),
            "target_module_sha": file_sha(HERE / f"{args.target_module}.py"),
            "corpus_sha": file_sha(HERE / "corpus.py"),
            "target_language": "python-stub (stands in for hermes-rs)",
        },
        "host": {
            "repo_head": repo_sha(),
            "python": sys.version.split()[0],
            "platform": platform.platform(),
        },
        "results": results,
    }

    out = Path(args.out)
    out.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")

    print(
        f"paired-run: total={total} passed={passed} failed={failed} "
        f"skip_excluded={skipped} -> {'OK' if ok else 'NOT OK'}"
    )
    print(f"harness_sha={doc['harness_sha']} repo_head={doc['host']['repo_head'][:12]}")
    print(f"wrote {out}")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
