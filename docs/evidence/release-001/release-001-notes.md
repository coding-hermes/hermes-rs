# RELEASE-001 — release surface, evidence notes

Row: `RELEASE-001` (release surface: driver, changelog, tag-triggered CI,
checksum manifest, release docs).
Trace: `ch:trace RELEASE-001 | spec=specs/COMPATIBILITY_MATRIX.md | evidence=docs/evidence/release-001/`

## What exists now

| Surface | Path | What it is |
|---|---|---|
| Release driver | `scripts/release.sh` | Reads `[package] version` from `Cargo.toml`, derives `v<version>`, supports `--dry-run` (plan only) and a real cut (`--confirm-readiness <row>`), and refuses on a dirty tree, an existing local or origin tag, an already-released version, an empty `[Unreleased]`, or a missing readiness acknowledgment. Covers version-stamp verification, the dated CHANGELOG section, release notes, the host artifact build and `SHA256SUMS`. Never pushes and never publishes. |
| Changelog | `CHANGELOG.md` | Keep-a-Changelog shape with `## [Unreleased]` at the top, seeded from this repository's own gates and history. The dated heading a cut writes is `## [<version>] - <YYYY-MM-DD>`. |
| Publish workflow | `.github/workflows/release.yml` | `push: tags: ['v*']` and `workflow_dispatch` (tag input for replay) only; `contents: write`; cross-compiles both binaries for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, generates and verifies `SHA256SUMS`, publishes via `softprops/action-gh-release`. Does not run on pushes to `main`. |
| Operator doc | `docs/releases.md` | Cut and publish procedure, exact artifact names, verification commands, rollback path, required CI state, and the standing gate set. |
| Evidence | this directory | `release-dry-run.log`, `release-refusals.log`, and this page. |

No tag and no artifact were created by this row. `git tag | wc -l` is `0`
before and after, and nothing was pushed or published.

## How the evidence was produced

Both logs are captured output of the commands shown inside them, run from a
clean tree at `/home/kara/hermes-rs` with stdout and stderr redirected verbatim.
Nothing was written by hand. The trace marker printed at the top of each log is
emitted by the command sequence itself, and each log ends with the assertions
that made the run self-checking (expected exit codes, zero tags, unchanged
HEAD, clean tree).

The logs were captured while the working tree was clean; the two log files were
added to this same commit afterwards. That is why the dry run reports a clean
worktree instead of reporting the untracked logs themselves.

- `release-dry-run.log` — `scripts/release.sh --dry-run` followed by
  `echo "rc=$?"`, `git tag | wc -l` and `git status --porcelain`. It shows the
  plan (version stamp, the exact CHANGELOG section a cut would write, the
  artifact list, the `SHA256SUMS` plan) and the two path checks the dry run
  exercises into a temporary directory: the CHANGELOG transform and the
  manifest line format. Exit code `0`; the tree is byte-identical afterwards.
- `release-refusals.log` — three refusals, each with its exit code:
  1. dirty tree (an untracked probe file, removed immediately after);
  2. existing tag (a temporary local `v0.1.0` created for the probe and
     deleted immediately after, with `git tag | wc -l` shown as `0` again);
  3. a clean tree and no tag at the current `0.1.0` stamp — the driver refuses
     because a real cut requires `--confirm-readiness <row>`. This is the
     documented behaviour for a bare invocation: refuse with an explicit
     message, never a silent no-op.

## What a cut would do

Given a clean `main`, an absent tag, a non-empty `[Unreleased]` and a named
readiness row, `scripts/release.sh --confirm-readiness <row>` would: run
`cargo fmt`/`clippy`/`test`; build both binaries in release mode for the host
target; write `target/release-artifacts/<tag>/` (artifacts,
`RELEASE-NOTES-<tag>.md`, verified `SHA256SUMS`); move the `[Unreleased]` body
under `## [<version>] - <date>` in `CHANGELOG.md`; commit only `CHANGELOG.md`
with subject `release: <tag>`; and create an annotated tag locally. Pushing that
tag is a separate operator step that runs the publish workflow.

The dry run shows all of it and changes nothing, which is why it is the
evidence shipped here.

## What is still gated

This row does not authorize a release and cannot produce one:

- The readiness sweep must return **GO** for the version and commit. The driver
  records the row you name; it cannot decide readiness.
- The paired upstream-harness comparative run (AC-2/AC-9) has not been executed
  and the corresponding compatibility matrix row is `Untriaged`, so no parity
  claim may accompany a release.
- Install/upgrade/rollback (AC-6) is unimplemented and `Untriaged`; the
  clean-machine install leg is blocked; the fresh-system QA battery has not run.
- `cargo audit` is clean apart from the disclosed `RUSTSEC-2026-0253` warning
  under `SEC-HRS-001`, and branch protection on `main` remains an open owner
  decision.

## No-claim rule

hermes-rs is an independent Rust project. It is not an official Nous Research
release, not affiliated with or endorsed by Nous Research, and not a drop-in
replacement for Hermes Agent. Nothing in this directory, the driver, the
changelog or the workflow states a compatibility, parity or performance claim;
such a claim may only rest on a repeatable artifact committed under
`docs/evidence/`. The publish workflow prepends the same disclaimer to every
release body it generates.
