#!/usr/bin/env python3
"""Shared, deterministic upstream-change classification for HRS-004.

Used by ``scripts/track_upstream_release.sh`` (snapshot assembly) and
``scripts/check_release_parity.py`` (parity gate). Everything here is pure:
no network, no clock, no filesystem access — only path strings in, stable
structures out. Deterministic output is a hard requirement: re-running the
tools on the same inputs must produce byte-identical artifacts.

Classification is prefix/extension based and order-sensitive: the FIRST
matching rule wins. The rule order below is part of the contract; do not
reorder without re-baselining committed snapshots and PARITY.md.
"""

from __future__ import annotations

import re
from pathlib import PurePosixPath

# --- Compatibility pin and upstream identity (single source; the snapshot
# builder imports these). Pinned by HRS-001: specs/COMPATIBILITY_MATRIX.md
# and specs/UPSTREAM_BASELINE.md record the provenance.
UPSTREAM_REPO = "NousResearch/hermes-agent"
PIN_TAG = "v0.22.0"
PIN_COMMIT = "e06df9c2eae6901f4c88f464e0ae6e753c0428b8"

# Areas that carry a STANDING no-action rationale in the parity gate.
# Every area NOT listed here becomes a parity item (PARI-<n>, pending
# triage) — the gate defaults to "needs an owner", never to "safe".
NO_ACTION_RATIONALES: dict[str, str] = {
    "docs": (
        "Upstream documentation/website content only; no runtime contract "
        "for hermes-rs."
    ),
    "locales": ("Translation strings only; no runtime contract for hermes-rs."),
    "ci": (
        "Upstream CI configuration (.github workflows); hermes-rs CI is "
        "maintained separately in this repository."
    ),
    "contributors": (
        "Upstream contributor-process documentation; no runtime contract for hermes-rs."
    ),
}

_TEST_BASENAME = re.compile(r"(^|_)(test|tests)(\.|_|$)|\.(test|spec)\.")

_PACKAGING_BASENAMES = {
    "dockerfile",
    "package.json",
    "package-lock.json",
    ".npmrc",
    "poetry.lock",
    "uv.lock",
    "requirements.txt",
    "requirements-dev.txt",
    "cargo.toml",
    "cargo.lock",
    "rust-toolchain.toml",
}

_CONFIG_EXTENSIONS = {".yaml", ".yml", ".toml", ".ini", ".cfg", ".conf", ".env"}

_SOURCE_EXTENSIONS = {
    ".py",
    ".rs",
    ".go",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".mjs",
    ".cjs",
    ".sh",
    ".bash",
    ".c",
    ".h",
    ".cpp",
    ".hpp",
    ".java",
    ".kt",
    ".rb",
}

_DOC_EXTENSIONS = {".md", ".rst", ".adoc", ".txt"}


def classify_path(path: str) -> str:
    """Return the change area for one upstream file path (first match wins)."""
    parts = [p.lower() for p in PurePosixPath(path).parts]
    basename = parts[-1] if parts else ""
    ext = PurePosixPath(basename).suffix
    stem = basename[: -len(ext)] if ext else basename

    # migrations: any path component containing "migration" (catches
    # "migrations/", "hermes_state_migrations/", "automigrations/", ...),
    # or a migration-shaped filename.
    if any("migration" in p for p in parts):
        return "migrations"

    # tests: upstream test trees or test/spec-shaped basenames.
    if parts and parts[0] in {"tests", "tests-js"}:
        return "tests"
    if _TEST_BASENAME.search(basename):
        return "tests"

    # ci: upstream GitHub workflow/config tree.
    if parts and parts[0] == ".github":
        return "ci"

    # locales: translation trees anywhere in the path.
    if (parts and parts[0] in {"locales", "i18n"}) or "i18n" in parts:
        return "locales"

    if parts and parts[0] == "contributors":
        return "contributors"

    # packaging: manifests/lockfiles/installers (install/upgrade behavior
    # is an in-scope matrix surface, so packaging is NOT a no-action area).
    if (
        basename in _PACKAGING_BASENAMES
        or stem.startswith("dockerfile")
        or (parts and parts[0] == "pm")
    ):
        return "packaging"

    # config: structured configuration files (after ci/packaging so their
    # workflow YAML and manifests keep their own areas).
    if ext in _CONFIG_EXTENSIONS or basename.endswith(".example"):
        return "config"

    # docs: prose/documentation content.
    if ext in _DOC_EXTENSIONS or (parts and parts[0] in {"docs", "doc", "website"}):
        return "docs"

    # source: executable/implementation code and operational scripts.
    if (
        ext in _SOURCE_EXTENSIONS
        or basename in {"makefile", "justfile"}
        or (parts and parts[0] == "scripts")
    ):
        return "source"

    return "other"


def release_version(name: str) -> tuple[int, ...]:
    """SemVer-ish sort key for a tag/release name ("v1.2.3" -> (1, 2, 3)).

    Tolerates suffixes (v1.2.3-rc1 -> (1, 2, 3)); non-numeric heads sort 0.
    """
    core = name[1:] if name.startswith("v") else name
    parts = core.split(".")
    head: list[int] = []
    for part in parts:
        digits = ""
        for ch in part:
            if ch.isdigit():
                digits += ch
            else:
                break
        head.append(int(digits) if digits else 0)
    return tuple(head)


def bucket_for(path: str) -> str:
    """Deterministic sub-bucket for a changed file: its top-level directory."""
    parts = PurePosixPath(path).parts
    if len(parts) <= 1:
        return "<root>"
    return parts[0]


def churn(entry: dict[str, object]) -> int:
    """Total observed line churn for one changed-file entry."""
    return int(entry.get("additions", 0) or 0) + int(entry.get("deletions", 0) or 0)


def top_paths(entries: list[dict[str, object]], limit: int = 3) -> list[str]:
    """Most-changed paths, ties broken by path name. Deterministic."""
    ranked = sorted(entries, key=lambda e: (-churn(e), str(e.get("path", ""))))
    return [str(e.get("path", "")) for e in ranked[:limit]]


# Descriptive hints mapping changed paths to COMPATIBILITY_MATRIX.md
# surfaces. Hints are triage AIDS surfaced in PARITY.md; they never assert
# parity by themselves. Surface names must match the matrix verbatim.
SURFACE_HINTS: list[tuple[str, tuple[str, ...]]] = [
    ("CLI and process lifecycle", ("hermes_cli/", "cli.py", "batch_runner.py")),
    (
        "Configuration and environment resolution",
        ("hermes_cli/config", "cli-config.yaml.example", ".env.example"),
    ),
    ("Agent loop / orchestration", ("agent/", "run_agent.py")),
    (
        "Provider and model routing",
        (
            "agent/provider",
            "agent/route_binding",
            "agent/fallback_cooldown",
            "agent/credential_pool",
            "agent/model_metadata",
            "providers/",
        ),
    ),
    (
        "Tools and tool-call protocols",
        ("tools/", "acp_adapter/", "toolsets.py", "agent/tool_"),
    ),
    ("MCP integrations", ("mcp",)),
    (
        "Gateway / API / streaming contracts",
        ("gateway/", "tui_gateway/", "agent/stream_delivery", "agent/chat_completion"),
    ),
    (
        "Memory / session / history semantics",
        ("hermes_state", "agent/memory_", "agent/compaction", "agent/session"),
    ),
    (
        "Upstream test framework and comparative scoring",
        ("tests/", "conftest.py", "scripts/run_tests.sh", "pyproject.toml"),
    ),
]

# Matrix surfaces that carry no upstream-path hints.
_HINTLESS_SURFACES = {
    "Persistence and database access",
    "Python libraries / extensions",
    "Install, upgrade, and release behavior",
    "Observability, limits, and resource controls",
}


def related_surfaces(paths: list[str]) -> list[str]:
    """Matrix surfaces whose path hints match any of ``paths`` (sorted)."""
    matched: set[str] = set()
    for surface, prefixes in SURFACE_HINTS:
        for path in paths:
            if any(path.startswith(prefix) or path == prefix for prefix in prefixes):
                matched.add(surface)
                break
    return sorted(matched)
