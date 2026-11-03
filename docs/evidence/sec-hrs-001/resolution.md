# SEC-HRS-001 — RustSec zstd advisories resolution via lockfile-only update

| Field | Value |
|---|---|
| Task | SEC-HRS-001 (dependency-security) |
| Date | 2026-11-03 |
| Base | `ab8e7c12e2b9b4ffdbc60422b75209267ae515bc` (branch `wt/SEC-HRS-001`, worktree) |
| Scope | Resolve all four cargo audit vulnerabilities; no source, test, docs, release, or QA changes |
| Mechanism | `cargo update --precise` on exactly the three advisory packages (Cargo.lock only, no manifest edit, no checksum hand-edit, no ignore/allowlist) |
| Compatibility claim | NONE (per `specs/HERMES_RS_COMPATIBILITY.md`; no drop-in/performance/parity claim is derived from this work) |

## 1. Findings reproduced (before)

`cargo audit` at base with advisory DB 1403 entries
(last-updated 2026-11-03T10:44:19+01:00) reported 4 vulnerabilities and exit
code 1. Raw JSON: `cargo-audit-before.json` in this directory.

| Advisory | Package | Version at base | Patched versions | CVSS category |
|---|---|---|---|---|
| RUSTSEC-2026-0391 | zstd | 0.13.3 | ^0.13.4, >=0.14.3 | memory-exposure (abandoned-stream context reuse leaks prior plaintext into the next stream) |
| RUSTSEC-2026-0403 | zstd-safe | 7.3.0 | ^7.4.0, >=8.2.0 | unsound |
| RUSTSEC-2026-0398 | zstd-safe | 7.3.0 | ^7.4.0, >=8.2.0 | unsound |
| RUSTSEC-2026-0410 | zstd-sys | 2.1.1+zstd.1.5.7 | >=2.1.2 | unsound |

Live dependency path (identical single path for all four; `cargo tree -e
normal,build --all-features -i <pkg>`, reproduced 2026-11-03):

```
zstd v0.13.3
└── tantivy-sstable v0.7.0
    └── tantivy-columnar v0.7.0
        └── tantivy v0.26.2
            └── turso_core v0.8.3
                ├── turso v0.8.3
                │   └── hermes-rs v0.1.0   ← feature `turso` (Cargo.toml [features])
                └── turso_sync_sdk_kit / turso_sdk_kit / turso_sync_engine (all under turso_core)
```

- zstd-safe 7.3.0 sits between zstd and the two zstd-safe advisories'
  affected functions; zstd-sys 2.1.1 sits below zstd-safe. Both are reached
  only through this same chain.
- **Feature scope:** the path exists only under the OPTIONAL `turso`
  feature (`turso = ["dep:turso", "dep:tokio"]`, never a default;
  specs/backend-evaluation.md verdict: not adopted). Without the feature,
  `cargo tree -e normal -i zstd` fails with "package ID specification
  `zstd` did not match any packages" — the default build does not compile
  zstd at all. The vulnbuild surface is therefore optional-only, and this
  work changes nothing about that boundary: Turso remains optional and
  not adopted, stock SQLite (rusqlite bundled) is untouched.

## 2. Resolution

All patched versions existed unyanked on crates.io (verified via the
crates.io API with a proper User-Agent: zstd 0.13.4, zstd-safe 7.4.0,
zstd-sys 2.1.2+zstd.1.5.7). Because each patched release is semver-
compatible with the in-use version, plain Cargo lockfile mechanisms reach
them — no manifest change, no vendor, no hand-edited checksum:

```
cargo update -p zstd-sys@2.1.1+zstd.1.5.7 --precise 2.1.2+zstd.1.5.7   # rc=0
cargo update -p zstd-safe@7.3.0 --precise 7.4.0                        # rc=0
cargo update -p zstd@0.13.3 --precise 0.13.4                           # rc=0
```

`git diff --stat` after the updates: `Cargo.lock | 14 ++++++++---` — the
only changed file, with exactly three version/checksum pairs swapped and
zero other packages moved. The bundled zstd C library stays 1.5.7
(zstd-sys 2.1.2+zstd.1.5.7); only the Rust wrapper layers changed.

## 3. Audit after update

`cargo audit` (same advisory DB, 1403 entries): 0 vulnerabilities, exit
code 0. Raw JSON: `cargo-audit-after-update.json` in this directory
(`vulnerabilities.count == 0`, `warnings: none` in JSON; the CLI prints
one informational `unsound` notice, see §5).

## 4. Gates (all fresh, 2026-11-03, this worktree)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | exit 0 |
| `cargo test --workspace --all-features` | exit 0 (incl. HRS-009 backend parity battery on both backends — the turso feature still builds and its tests still pass on the updated chain) |
| `cargo audit` | exit 0, 0 vulnerabilities |
| `python3 scripts/check-project-artifacts.py` | exit 0 |
| `python3 scripts/check_skill_claims.py` | exit 0 |
| `git diff --check` | exit 0 |

## 5. Remaining notices (explicit, not ignored)

- `RUSTSEC-2026-0253` (unsound; lru 0.16.4, "potential use-after-free due
  to lack of panic safety in `LruCache::pop()`", also via turso_core →
  tantivy) remains as an **informational, allowed warning** — cargo audit
  exits 0 and counts 0 vulnerabilities with it present. No `.cargo/audit.toml`
  or `[ignore]` entries were added anywhere; nothing was silenced.
  lru 0.16.4 is already the newest 0.16.x line at resolution time and the
  advisory lists no patched release, so there is no compatible lockfile
  bump that removes it. It is not one of the four vulnerabilities in
  SEC-HRS-001's scope and is disclosed here rather than suppressed.

## 6. Repro

From a clean checkout of the commit carrying this file:

```
cargo audit                       # 0 vulnerabilities, exit 0
cargo tree -e normal --all-features -i zstd      # zstd 0.13.4 via tantivy-sstable ← tantivy ← turso_core ← turso (feature `turso`)
cargo tree -e normal -i zstd      # error: not in graph without the `turso` feature
```
