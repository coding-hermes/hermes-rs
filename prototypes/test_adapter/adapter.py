"""HRS-007 adapter boundary — the ONLY sanctioned seam between the paired
test runner and the Rust target.

Fail-closed rules (see specs/TEST_ADAPTER_CONTRACT.md section 6):

1. The adapter is SEALED against Python substitution: any attempt to
   import, call, or monkeypatch a Python implementation module through
   this boundary raises AdapterSealedError. A Python fallback therefore
   cannot produce a Rust pass — the contract's core requirement.
2. Target unavailability is FAIL-CLOSED: a missing/unraisable target
   yields target="unavailable" and status FAIL (or SKIP-EXCLUDED for
   vectors marked excluded), never a vacuous pass.
3. Comparison is exact (json equality) on every expect field, so any
   target defect flips the paired result.
"""

from __future__ import annotations

import importlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

# Modules a Python-fallback attempt would reach for. The seal blocks them.
_FORBIDDEN_FALLBACK_MODULES = (
    "providers",
    "agent.transports.chat_completions",
    "run_agent",
    "hermes_cli.models",
    "plugins.model_providers",
)


class AdapterSealedError(RuntimeError):
    """Raised when the boundary detects a Python-fallback attempt."""


def _assert_sealed() -> None:
    for mod in _FORBIDDEN_FALLBACK_MODULES:
        if mod in sys.modules:
            raise AdapterSealedError(
                f"adapter boundary is sealed: python module '{mod}' is "
                "imported in this process and could serve as a fallback "
                "implementation; a Rust pass must come from the Rust target"
            )


def compare(vector: dict, target_result: dict) -> dict:
    """Exact comparison of target output against the vector contract."""
    exp = vector["expect"]
    checks: dict[str, dict] = {}
    checks["url"] = {
        "expected": exp["url"],
        "actual": target_result.get("url"),
        "match": target_result.get("url") == exp["url"],
    }
    checks["wire_model"] = {
        "expected": exp["wire_model"],
        "actual": target_result.get("wire_model"),
        "match": target_result.get("wire_model") == exp["wire_model"],
    }
    body = target_result.get("body") or {}
    for key, want in exp.items():
        if key in ("url", "wire_model", "tool_message_index"):
            continue
        if key == "tool_message":
            idx = exp["tool_message_index"]
            got = (
                (body.get("messages") or [{}])[idx]
                if len(body.get("messages") or []) > idx
                else None
            )
            checks[key] = {"expected": want, "actual": got, "match": got == want}
        elif key.startswith("extra_body_options_"):
            field = key[len("extra_body_options_") :]
            got = ((body.get("extra_body") or {}).get("options") or {}).get(field)
            checks[key] = {"expected": want, "actual": got, "match": got == want}
        elif key.startswith("extra_body_"):
            field = key[len("extra_body_") :]
            got = (body.get("extra_body") or {}).get(field)
            checks[key] = {"expected": want, "actual": got, "match": got == want}
        elif key.startswith("supports_"):
            # profile capability flags are result-level, not body fields
            got = target_result.get(key)
            checks[key] = {"expected": want, "actual": got, "match": got == want}
        else:
            got = body.get(key)
            checks[key] = {"expected": want, "actual": got, "match": got == want}
    all_match = all(c["match"] for c in checks.values())
    return {"vector": vector["id"], "checks": checks, "match": all_match}


def run_paired(
    vector: dict, target_module: str = "rust_target", broken: bool | None = None
) -> dict:
    """Run one vector through the sealed boundary against the Rust target."""
    _assert_sealed()

    if vector.get("excluded"):
        return {
            "vector": vector["id"],
            "status": "SKIP-EXCLUDED",
            "reason": vector.get("exclusion_reason", "excluded"),
        }

    try:
        mod = importlib.import_module(target_module)
    except ImportError as exc:
        return {
            "vector": vector["id"],
            "status": "FAIL",
            "target": "unavailable",
            "reason": f"rust target unavailable: {exc}",
        }

    try:
        result = mod.rust_target_run(vector, broken=broken)
    except Exception as exc:  # target crash = paired FAIL, never a pass
        return {
            "vector": vector["id"],
            "status": "FAIL",
            "target": getattr(mod, "__name__", target_module),
            "reason": f"rust target raised: {type(exc).__name__}: {exc}",
        }

    verdict = compare(vector, result)
    return {
        "vector": vector["id"],
        "status": "PASS" if verdict["match"] else "FAIL",
        "target": getattr(mod, "__name__", target_module),
        "verdict": verdict,
    }


def load_corpus():
    import corpus  # local module, not a python-implementation fallback

    return corpus.VECTORS, corpus.VECTOR_IDS


def manifest_harness_sha() -> str:
    manifest = HERE.parent.parent / "specs" / "TEST_CLASSIFICATION_MANIFEST.json"
    data = json.loads(manifest.read_text(encoding="utf-8"))
    return data["harness_sha"]
