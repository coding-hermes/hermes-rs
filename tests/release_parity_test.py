#!/usr/bin/env python3
"""No-network fixture tests for the HRS-004 release-parity gate.

Drives ``scripts/check_release_parity.py`` against a fixed synthetic
snapshot (``tests/fixtures/release-parity/sample-snapshot.json``) and
proves:

1. a KNOWN upstream change (source-area edits to ``agent/conversation_loop.py``
   and ``hermes_cli/config.py``) surfaces as a parity item whose related
   matrix surfaces are hinted correctly;
2. the no-action path emits an explicit rationale (docs area);
3. output is deterministic (byte-identical PARITY.md across runs) and the
   ledger append path is idempotent;
4. the shared classifier maps representative upstream paths to the
   documented areas.

No network access is required or performed. Standard library only;
runs under pytest or directly (``python3 tests/release_parity_test.py``).
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from release_areas import classify_path  # noqa: E402

FIXTURE_DIR = REPO_ROOT / "tests" / "fixtures" / "release-parity"
SNAPSHOT_FIXTURE = FIXTURE_DIR / "sample-snapshot.json"
LEDGER_FIXTURE = FIXTURE_DIR / "expected-ledger.jsonl"
GATE = REPO_ROOT / "scripts" / "check_release_parity.py"


def run_gate(
    snapshot: Path, ledger: Path, parity: Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(GATE),
            "--snapshot",
            str(snapshot),
            "--ledger",
            str(ledger),
            "--parity",
            str(parity),
        ],
        capture_output=True,
        text=True,
        check=False,
    )


class ClassifierTests(unittest.TestCase):
    """The shared path classifier must match its documented rule order."""

    def test_known_paths_classify_to_documented_areas(self) -> None:
        cases = {
            "tests/agent/test_conversation_loop.py": "tests",
            "tests-js/unit/foo.test.ts": "tests",
            "agent/tool_executor_test.py": "tests",
            "apps/desktop/electron/main.spec.ts": "tests",
            "hermes_state_migrations/0007_add_turn_receipts.py": "migrations",
            "migrations/0002_idx.sql": "migrations",
            "agent/conversation_loop.py": "source",
            "hermes_cli/config.py": "source",
            # Test-shaped basename wins by documented rule order: the upstream
            # harness runner lands in the tests triage queue (its surface hint
            # still fires via related_surfaces -> "Upstream test framework").
            "scripts/run_tests.sh": "tests",
            "gateway/run.py": "source",
            "hermes_cli/config_effective.py": "source",
            "cli-config.yaml.example": "config",
            ".env.example": "config",
            "gateway/config.yaml": "config",
            "pyproject.toml": "config",
            ".github/workflows/ci.yaml": "ci",
            "locales/es.json": "locales",
            "apps/desktop/src/i18n/en.ts": "locales",
            "website/docs/user-guide/configuration.md": "docs",
            "README.md": "docs",
            "Dockerfile": "packaging",
            "package-lock.json": "packaging",
            "pm/install.sh": "packaging",
            "something_unheard_of.bin": "other",
        }
        for path, expected in cases.items():
            with self.subTest(path=path):
                self.assertEqual(classify_path(path), expected)


class ParityGateTests(unittest.TestCase):
    """End-to-end gate runs over the fixed fixture, with no network."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="hrs004-parity-test-")
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)
        self.ledger = self.tmp / "parity-ledger.md"
        self.parity = self.tmp / "PARITY.md"
        shutil.copy(LEDGER_FIXTURE, self.ledger)

    def test_known_change_becomes_parity_item_and_no_action_emits_rationale(
        self,
    ) -> None:
        result = run_gate(SNAPSHOT_FIXTURE, self.ledger, self.parity)
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        report = self.parity.read_text(encoding="utf-8")

        # Known source-area change surfaces as a parity item...
        self.assertIn("### PARI-102 — `source` area (v9.9.9-fixture)", report)
        # ...with the matrix surfaces its changed paths hint at.
        self.assertIn(
            "Related matrix surfaces (hinted by changed paths; triage must"
            " confirm):** Agent loop / orchestration; CLI and process lifecycle;"
            " Configuration and environment resolution",
            report,
        )
        # A known migration lands as its own parity item (in-scope surface).
        self.assertIn("### PARI-101 — `migrations` area (v9.9.9-fixture)", report)
        self.assertIn(
            "Related matrix surfaces (hinted by changed paths; triage must"
            " confirm):** Memory / session / history semantics",
            report,
        )
        # Tests-area change is tracked, never silently dropped.
        self.assertIn("### PARI-103 — `tests` area (v9.9.9-fixture)", report)

        # No-action path: docs area recorded WITH its rationale, not silence.
        self.assertIn("## No-action areas (1)", report)
        self.assertIn(
            "Upstream documentation/website content only; no runtime contract",
            report,
        )

        # Claim discipline: report carries the no-claim framing.
        self.assertIn("not parity claims", report)
        self.assertIn("pending triage", report)

        # The ledger was already complete for this tag: no appends.
        self.assertEqual(
            self.ledger.read_bytes(),
            LEDGER_FIXTURE.read_bytes(),
            "gate appended rows for a fully-ledgered tag",
        )

    def test_gate_is_deterministic_and_idempotent(self) -> None:
        first = run_gate(SNAPSHOT_FIXTURE, self.ledger, self.parity)
        self.assertEqual(first.returncode, 0, msg=first.stderr)
        first_bytes = self.parity.read_bytes()
        first_ledger = self.ledger.read_bytes()

        second = run_gate(SNAPSHOT_FIXTURE, self.ledger, self.parity)
        self.assertEqual(second.returncode, 0, msg=second.stderr)
        self.assertEqual(
            self.parity.read_bytes(), first_bytes, "PARITY.md is not deterministic"
        )
        self.assertEqual(
            self.ledger.read_bytes(), first_ledger, "ledger is not idempotent"
        )
        self.assertIn("written:", first.stdout)
        self.assertIn("unchanged:", second.stdout)

    def test_empty_ledger_gets_sequential_appends(self) -> None:
        fresh_ledger = self.tmp / "fresh-ledger.jsonl"
        result = run_gate(SNAPSHOT_FIXTURE, fresh_ledger, self.tmp / "PARITY-fresh.md")
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        rows = [
            json.loads(line)
            for line in fresh_ledger.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        self.assertEqual(
            [(r["id"], r["area"]) for r in rows],
            [
                ("PARI-001", "migrations"),
                ("PARI-002", "source"),
                ("PARI-003", "tests"),
            ],
        )
        for row in rows:
            self.assertEqual(row["tag"], "v9.9.9-fixture")
            self.assertEqual(row["status"], "pending triage")
            self.assertEqual(row["task"], "HRS-004")
            self.assertNotIn("{area}", row["action"])

    def test_snapshot_fixture_schema_is_current(self) -> None:
        data = json.loads(SNAPSHOT_FIXTURE.read_text(encoding="utf-8"))
        self.assertEqual(data["schema"], "hermes-rs/upstream-release-snapshot")
        self.assertIn("top_paths_by_area", data["compare"]["summary"])
        self.assertIn("url", data["compare"])
        self.assertIn("compare_url", data["area_pointers"])
        self.assertTrue(data["compare"]["file_list_complete"])


if __name__ == "__main__":
    unittest.main()
