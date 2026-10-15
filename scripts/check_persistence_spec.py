#!/usr/bin/env python3
"""Conformance check for specs/persistence-shim.md (HRS-002).

Verifies, for the persistence shim + stock-SQLite compatibility spec:

1. Required sections exist (§1-§8 plus the inventory subsections).
2. The upstream pin is cited (v0.22.0 @ e06df9c2...).
3. The inventory carries source references (hermes_state*/ path:line cites).
4. specs/COMPATIBILITY_MATRIX.md points back at the spec.
5. NO forbidden claim appears: an affirmative speed claim, a Turso drop-in
   claim, or a PostgreSQL support claim, unless the sentence containing it
   defers the matter to HRS-009 pending measurements (or negates it).

Exit 0 = pass; nonzero with a diagnostic per failure.

Usage: python3 scripts/check_persistence_spec.py [repo_root]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

SPEC_REL = Path("specs/persistence-shim.md")
MATRIX_REL = Path("specs/COMPATIBILITY_MATRIX.md")

REQUIRED_SECTIONS = [
    "## 1. Purpose and scope",
    "## 2. Upstream persistence inventory",
    "### 2.1 Storage surface and file set",
    "### 2.2 Tables",
    "### 2.3 Indexes, views, triggers",
    "### 2.4 SQL operation inventory",
    "### 2.5 Transaction boundaries",
    "### 2.6 FTS5 use",
    "### 2.7 WAL / journal-mode / PRAGMA policy",
    "### 2.8 Identifiers",
    "### 2.9 Migrations",
    "### 2.10 Backup / restore / recovery",
    "### 2.11 Cross-process behavior",
    "## 3. Rust persistence shim API (engine-neutral)",
    "## 4. Stock-SQLite default backend compatibility contract",
    "## 5. Compatibility fixtures",
    "## 6. Session-store vertical slice (HRS-010 contract)",
    "## 7. Deferred behavior (HRS-009 and later)",
    "## 8. Verification",
]

# Inventory items must be grounded: the inventory body (§2) must cite upstream
# paths and the pin.
INVENTORY_SOURCE_PATTERNS = [
    r"hermes_state[_a-z]*\.py",
    r"hermes_cli/backup[a-z_]*\.py",
    r"e06df9c2eae6901f4c88f464e0ae6e753c0428b8",
    r"SCHEMA_VERSION\s*=\s*31",
]

# The engine-leak rule: no candidate-engine types in the CALLER-FACING API
# section. Turso/Postgres may only appear as deferred-evaluation discussion,
# never as API surface.
API_SECTIONS = [
    "## 3. Rust persistence shim API (engine-neutral)",
    "## 4. Stock-SQLite default backend compatibility contract",
]

# Forbidden claims. Each pattern matches an AFFIRMATIVE claim; the sentence is
# allowed only when it also carries a deferral/negation phrase.
FORBIDDEN = [
    (re.compile(r"\bturso\s+drop[- ]?in\b", re.I), "Turso drop-in claim"),
    (re.compile(r"\bfaster than\b", re.I), "speed comparison claim ('faster than')"),
    (re.compile(r"\bspeedup\b", re.I), "speedup claim"),
    (
        re.compile(r"\bpostgres(ql)?\s+(is\s+)?supported\b", re.I),
        "PostgreSQL support claim",
    ),
    (re.compile(r"\bsupports?\s+postgres(ql)?\b", re.I), "PostgreSQL support claim"),
    (
        re.compile(
            r"\bturso\s+(is\s+)?(a\s+)?(viable|the\s+default|our\s+default)\b", re.I
        ),
        "Turso adoption/default claim",
    ),
]

DEFERRAL_MARKERS = [
    "deferred to hrs-009 pending measurements",
    "deferred to hrs-009",
    "pending measurements",
    "no claim",
    "not asserted",
    "does not make",
    "no speed",
    "not implied",
    "remains an optional",
    "optional deployment choice",
    "out of hrs-010's scope",
    "owned by the hrs-009 gate",
    "deferred to hrs-009's",
]

PIN_PATTERN = re.compile(r"e06df9c2eae6901f4c88f464e0ae6e753c0428b8")


def split_sentences(text: str) -> list[str]:
    # Split on sentence enders, keeping table rows / list items as their own
    # units (a table cell with a claim must carry its own deferral).
    raw = re.split(r"(?<=[.!?])\s+|\n", text)
    return [s.strip() for s in raw if s.strip()]


def check_spec(repo: Path) -> list[str]:
    errors: list[str] = []
    spec_path = repo / SPEC_REL
    if not spec_path.exists():
        return [f"missing spec file: {SPEC_REL}"]
    spec = spec_path.read_text(encoding="utf-8")

    # 1. Required sections.
    for section in REQUIRED_SECTIONS:
        if section not in spec:
            errors.append(f"missing required section: {section!r}")

    # 2. Pin citation.
    if not PIN_PATTERN.search(spec):
        errors.append(
            "spec does not cite the pinned commit e06df9c2eae6901f4c88f464e0ae6e753c0428b8"
        )

    # 3. Inventory grounding.
    inventory = spec.split("## 2. Upstream persistence inventory", 1)[-1].split(
        "## 3. Rust persistence shim API", 1
    )[0]
    for pattern in INVENTORY_SOURCE_PATTERNS:
        if not re.search(pattern, inventory):
            errors.append(
                f"inventory section lacks required source-grounding pattern: {pattern!r}"
            )

    # 4. Matrix points back at the spec.
    matrix_path = repo / MATRIX_REL
    if not matrix_path.exists():
        errors.append(f"missing matrix file: {MATRIX_REL}")
    else:
        matrix = matrix_path.read_text(encoding="utf-8")
        if "persistence-shim.md" not in matrix:
            errors.append(
                "COMPATIBILITY_MATRIX.md does not reference specs/persistence-shim.md"
            )
        if not re.search(r"HRS-002", matrix):
            errors.append("COMPATIBILITY_MATRIX.md has no HRS-002 row marker")

    # 5. Forbidden claims outside deferral contexts.
    for i, line in enumerate(spec.splitlines(), start=1):
        for pattern, label in FORBIDDEN:
            if not pattern.search(line):
                continue
            # Allow when the same line, sentence, or heading context defers/negates.
            context = line
            if any(marker in context.lower() for marker in DEFERRAL_MARKERS):
                continue
            # Fallback: look at the enclosing ±2 lines for a section header that
            # is explicitly about deferral.
            window_start = max(0, i - 3)
            window = "\n".join(spec.splitlines()[window_start : i + 2]).lower()
            if any(
                marker in window
                for marker in (
                    "deferred behavior",
                    "explicitly deferred",
                    "non-goals",
                    "deferred to hrs-009",
                )
            ):
                continue
            errors.append(
                f"{SPEC_REL}:{i}: forbidden claim ({label}) without a "
                f"deferred-to-HRS-009/negation context: {line.strip()[:160]}"
            )

    # 6. Engine-leak rule: API/compat sections must not name candidate engines
    #    as API surface (mention only inside deferred/N/A framing is not
    #    allowed AT ALL in the caller-facing API section — §3/§4 concern the
    #    stock backend and the neutral contract).
    for section_header in API_SECTIONS:
        body = spec.split(section_header, 1)[-1]
        body = body.split("\n## ", 2)[0] if "\n## " in body else body
        # Cut at the next '## ' heading.
        cut = re.search(r"\n## \d", body)
        if cut:
            body = body[: cut.start()]
        for engine in ("turso", "limbo", "postgres"):
            if re.search(rf"\b{engine}", body, re.I):
                errors.append(
                    f"engine-leak: caller-facing API section {section_header!r} "
                    f"mentions {engine!r} — candidate engines must not leak into "
                    f"the shim API surface"
                )

    return errors


def main() -> int:
    repo_arg = sys.argv[1] if len(sys.argv) > 1 else "."
    repo = Path(repo_arg).resolve()
    errors = check_spec(repo)
    if errors:
        print(f"FAIL: {len(errors)} conformance error(s) in persistence spec:")
        for err in errors:
            print(f"  - {err}")
        return 1
    print(
        "OK: specs/persistence-shim.md — sections present, pin cited, "
        "inventory source-grounded, matrix cross-linked, no forbidden "
        "speed/drop-in/support claims."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
