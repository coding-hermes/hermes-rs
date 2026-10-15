"""HRS-007 fail-closed adapter prototype tests.

Proves the acceptance-criteria behavior of the prototype:
  1. Broken Rust target -> upstream-mirrored paired cases FAIL.
  2. No Python fallback can produce a Rust pass (sealed boundary).
  3. A correct target flips every paired result to PASS.
  4. Missing target is fail-closed FAIL, never a vacuous pass.

Run: python3 -m pytest prototypes/test_adapter/test_fail_closed.py -v
(repo root; the test file manages sys.path itself — no repo package
imports, so this suite is dependency-free).
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))

import adapter  # noqa: E402
import corpus  # noqa: E402
import rust_target  # noqa: E402


def test_corpus_covers_declared_vectors():
    vectors, ids = adapter.load_corpus()
    assert len(vectors) >= 12
    assert len(ids) == len(vectors) == len(set(ids))
    for vec in vectors:
        assert vec["upstream_case"], vec["id"]
        assert "url" in vec["expect"] and "wire_model" in vec["expect"]


def test_manifest_reconciles_and_pins():
    manifest = json.loads(
        (REPO / "specs" / "TEST_CLASSIFICATION_MANIFEST.json").read_text(
            encoding="utf-8"
        )
    )
    totals = manifest["totals"]
    by_class = totals["by_class"]
    assert totals["collected"] == totals["classified"] == sum(by_class.values())
    assert len(manifest["cases"]) == totals["collected"]
    assert manifest["harness_sha"] == "e06df9c2eae6901f4c88f464e0ae6e753c0428b8"
    # every case id unique
    case_ids = [c["id"] for c in manifest["cases"]]
    assert len(case_ids) == len(set(case_ids))


def test_every_vector_upstream_case_is_in_manifest():
    """Corpus vectors must mirror cases the manifest classified observable."""
    manifest = json.loads(
        (REPO / "specs" / "TEST_CLASSIFICATION_MANIFEST.json").read_text(
            encoding="utf-8"
        )
    )
    by_id = {c["id"]: c for c in manifest["cases"]}
    for vec in corpus.VECTORS:
        case = by_id.get(vec["upstream_case"])
        assert case is not None, f"{vec['upstream_case']} not in manifest"
        assert case["class"] == "observable", (
            f"{vec['upstream_case']} classified {case['class']}, corpus mirrors it anyway"
        )


def test_broken_target_fails_every_affected_vector():
    """AC: broken Rust target -> paired upstream-mirrored cases FAIL."""
    results = [adapter.run_paired(vec, broken=True) for vec in corpus.VECTORS]
    failed = [r for r in results if r["status"] == "FAIL"]
    assert failed, "broken target must fail at least the extra_body vectors"
    # the injected defect affects every vector that carries an extra_body
    # expectation AND the deepseek max_tokens leak vector
    ids_failed = {r["vector"] for r in failed}
    assert "ds-no-forced-max-tokens" in ids_failed
    assert "or-provider-prefs" in ids_failed  # extra_body.provider wrapped wrong
    # every FAIL carries a concrete check diff, not an empty verdict
    for r in failed:
        if "verdict" in r:
            bad = [k for k, v in r["verdict"]["checks"].items() if not v["match"]]
            assert bad, r


def test_correct_target_passes_all_vectors():
    """Flipping the stub to correct behavior flips every result to PASS."""
    results = [adapter.run_paired(vec, broken=False) for vec in corpus.VECTORS]
    assert all(r["status"] == "PASS" for r in results), [
        (
            r["vector"],
            [
                k
                for k, v in r.get("verdict", {}).get("checks", {}).items()
                if not v["match"]
            ],
        )
        for r in results
        if r["status"] != "PASS"
    ]


def test_default_stub_target_is_broken():
    """The shipped stub must be in its deliberately-broken state."""
    assert rust_target.BROKEN is True


def test_no_python_fallback_can_produce_a_rust_pass():
    """AC: the boundary is sealed - any Python implementation module
    present in the process makes run_paired raise, so a Python fallback
    cannot silently serve a Rust pass."""
    import types

    # Simulate a Python fallback under the real implementation's module
    # name (upstream `providers` is not importable from this repo, so a
    # registered stand-in is exactly what an attempted fallback looks like).
    sys.modules["providers"] = types.ModuleType("providers")

    try:
        try:
            adapter.run_paired(corpus.VECTORS[0], broken=False)
        except adapter.AdapterSealedError:
            pass
        else:
            raise AssertionError(
                "run_paired must refuse to run while a Python implementation "
                "module is imported in-process"
            )
    finally:
        sys.modules.pop("providers", None)

    # and with the module absent, the same call goes through the target
    res = adapter.run_paired(corpus.VECTORS[0], broken=False)
    assert res["status"] == "PASS"


def test_missing_target_is_fail_closed():
    """Target unavailable -> FAIL with target=unavailable, never PASS."""
    res = adapter.run_paired(
        corpus.VECTORS[0], target_module="no_such_rust_target_module"
    )
    assert res["status"] == "FAIL"
    assert res["target"] == "unavailable"


def test_crashing_target_is_fail_closed():
    """A target that raises yields FAIL with the exception recorded."""

    class Boom:
        @staticmethod
        def rust_target_run(vector, broken=None):
            raise RuntimeError("engine exploded")

    saved = sys.modules.get("boom_target")
    sys.modules["boom_target"] = Boom
    try:
        res = adapter.run_paired(corpus.VECTORS[0], target_module="boom_target")
    finally:
        if saved is None:
            sys.modules.pop("boom_target", None)
        else:
            sys.modules["boom_target"] = saved
    assert res["status"] == "FAIL"
    assert "engine exploded" in res["reason"]


def test_paired_run_cli_default_broken_exit_1():
    """The CLI against the default (broken) stub exits nonzero and says so."""
    proc = subprocess.run(
        [sys.executable, str(HERE / "paired_run.py")],
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert proc.returncode == 1, proc.stdout + proc.stderr
    assert "failed=" in proc.stdout
    out_path = HERE / "last_paired_run.json"
    doc = json.loads(out_path.read_text(encoding="utf-8"))
    assert doc["schema"] == "hermes-rs/paired-run/1"
    assert doc["denominator"]["reconciles"] is True
    assert doc["counts"]["failed"] > 0
    assert doc["harness_sha"] == "e06df9c2eae6901f4c88f464e0ae6e753c0428b8"
    # every failing result names its vector and check-level reasons
    for r in doc["results"]:
        if r["status"] == "FAIL":
            assert r["vector"] and r.get("verdict", r.get("reason"))


def test_paired_run_cli_correct_target_exit_0():
    """With --broken false the same CLI exits 0 with all vectors passing."""
    proc = subprocess.run(
        [
            sys.executable,
            str(HERE / "paired_run.py"),
            "--broken",
            "false",
            "--out",
            str(HERE / "last_paired_run_ok.json"),
        ],
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert proc.returncode == 0, proc.stdout + proc.stderr
    doc = json.loads((HERE / "last_paired_run_ok.json").read_text(encoding="utf-8"))
    assert doc["counts"] == {
        "passed": doc["denominator"]["total"],
        "failed": 0,
        "skip_excluded": 0,
        "other": 0,
    }
