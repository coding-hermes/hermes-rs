# Changelog

All notable changes to this project are documented in this file.

**This is an independent Rust project.** It is not an official Nous Research
release, it is not affiliated with or endorsed by Nous Research, and it is
**not a drop-in replacement** for Hermes Agent. Its implemented scope is a
stock-SQLite session store plus a stdio JSON-RPC adapter for a standard-library
Python client — no agent orchestration, no other subsystem. No compatibility,
parity, or performance claim is made beyond the evidence committed under
`docs/evidence/`. Upstream reference (not a dependency): [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent).

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Sections are `Added`, `Changed` and `Fixed`; the top section is always
`## [Unreleased]`. A dated heading — `## [<version>] - <YYYY-MM-DD>` — is
written only by `scripts/release.sh` at cut time, by moving the `Unreleased`
body under it. Entries are added by hand; the driver never invents release
content.

Nothing listed below has been released. The repository holds no tag and no
published artifact; everything under `Unreleased` is the current state of
`main`, seeded from the repository's own history and `specs/ROADMAP.md` gates.

## [Unreleased]

### Added

- Foundation scaffold and Rust workspace (Gate 4, HRS-008): a Cargo workspace
  with a pinned toolchain, the `hermes-rs` CLI binary, the `hermes_rs` library,
  non-vacuous tests, and an active Rust CI gate (format, lint, tests).
- Active CI gates (`.github/workflows/ci.yml`, `rust-ci.yml`,
  `release-parity.yml`): project-contract and board-integrity checks, Rust
  format/lint/test, and an upstream-parity fixture gate that publishes nothing.
- Persistence boundary and the first vertical slice (Gate 3 HRS-002, Gate 5
  HRS-010): `specs/persistence-shim.md` and a stock-SQLite session store behind
  an engine-neutral shim, covering the specification's §6.1 operations (open,
  ensure, append, get, search, end, usage) with verbatim upstream DDL,
  reconciliation, journal resolution, backup-API snapshots, quarantine
  admission, and an error taxonomy. Scope is deliberately narrower than
  Hermes: rotation, leases, outbox and the crash window are not implemented and
  are recorded as such in the parity report.
- Python interoperability boundary (Gate 6, HRS-003):
  `hermes-rs-session-adapter`, a newline-delimited JSON-RPC server over stdio
  (protocol `hermes-rs/session-adapter/1`) driven from a standard-library
  CPython client, with a documented scope statement and rejected alternatives
  in `specs/python-interop.md`.
- Upstream release tracking (HRS-004): `scripts/track_upstream_release.sh`,
  per-release snapshots under `docs/upstream-releases/`, a deterministically
  regenerated parity report, and a parity ledger.
- Optional backend evaluation (Gate 7, HRS-009): the `turso` Cargo feature and
  a parity battery/benchmark harness. Evaluation result at turso 0.8.3: **not
  adopted** — files containing FTS5 break its engine, so it cannot open real
  Hermes databases; stock SQLite remains the only default and the feature stays
  off by default. No speed claim is made.
- Measurement harness (HRS-005, HRS-006): scaffold and measured capture
  scripts with environment metadata and per-field null reasons, plus the
  recorded Python baseline profiles under `baseline/`.
- Release surface (RELEASE-001): `scripts/release.sh` (the release driver),
  this changelog, a tag-triggered publish workflow
  (`.github/workflows/release.yml`) that cross-compiles the two binaries for
  `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, emits
  `SHA256SUMS`, and publishes a GitHub Release, plus the operator procedure in
  `docs/releases.md`. Adding the surface cut no release: no tag exists and none
  was created.

### Changed

- The Rust crate's `bundled` SQLite feature pins the amalgamation so file-format
  behavior does not drift with the host's libsqlite3.
- The SIGKILL crash-window tests for the session store were made to exercise a
  real crash window rather than a vacuous one.

### Fixed

- Documented `session_model_usage` column order is checked against the DDL by
  `scripts/check-project-artifacts.py`, so prose cannot drift from the schema.
- The usage skill's claims about board rows are checked against the board by
  `scripts/check_skill_claims.py`, with recorded negative controls.
- Skipped tests (for example when the `sqlite3` CLI is absent) report a reason
  instead of failing silently.

### Known gaps

- The paired upstream-harness comparative run required by the compatibility
  spec's AC-2 and AC-9 has not been executed; the corresponding compatibility
  matrix row is `Untriaged`, so no parity claim follows from this file.
- No install, upgrade, or rollback implementation exists (the matrix row is
  `Untriaged`); the repository installs from a source checkout only.
- Every subsystem other than persistence remains "Pending Rust implementation"
  in `specs/COMPATIBILITY_MATRIX.md`.
