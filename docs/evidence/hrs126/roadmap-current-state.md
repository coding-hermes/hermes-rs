# HRS-126 — ROADMAP current-state correction

## What was wrong

`specs/ROADMAP.md` §Current state (last edited 2026-10-14, `db49c65`, the commit
that split the Rust foundation from the first subsystem) contained two claims
that were materially false against HEAD `4946822`:

1. "There is not yet a Rust crate, executable, or Rust test run." — the repo
   ships a Cargo workspace with two executables and 14 integration test targets.
2. "The current resource-profile numbers are preliminary: the capture
   implementation needs correction before they can justify architecture or
   performance claims." — HRS-006 repaired the capture implementation
   (`UPSTREAM_BASELINE.md` §6, captured records `baseline/hrs006-*.json`).

The second half of that sentence — that the numbers cannot justify architecture
or performance claims — is still true and was preserved in the rewritten text.

The defect class is the same one DF-HRS-118 fixed for `README.md`: a
present-tense document describing HEAD while naming a state several gates old.
DF-HRS-118 covered the README only; the roadmap, which is the document that
sequences delivery, was never re-measured after HRS-006 through HRS-010 landed.

## Measurement (re-runnable)

Raw output of the audit at HEAD `4946822`: [`audit.log`](audit.log).

| Claim in the rewritten section | Command that proves it | Result |
|---|---|---|
| Cargo workspace, two executables | `grep -n '^\[\[bin\]\]' -A2 Cargo.toml` | `hermes-rs`, `hermes-rs-session-adapter` |
| ~4.9k lines under `src/` | `find src -name '*.rs' \| xargs wc -l \| tail -1` | `4942 total` |
| 14 integration test targets | `ls tests/*.rs \| wc -l` | `14` |
| CI gate is fmt + clippy + tests | `grep -nE 'cargo (fmt\|clippy\|test)' .github/workflows/rust-ci.yml` | lines 37, 39, 41 |
| fmt clean | `cargo fmt --all -- --check` | exit 0 |
| clippy clean | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | `Finished` (no diagnostics) |
| 65 tests pass | `cargo test --workspace --all-features` (aggregated `test result:` lines) | `passed=65 failed=0 ignored=0` |
| audit: 0 vulnerabilities, 1 disclosed warning | `cargo audit` | `warning: 1 allowed warning found` — RUSTSEC-2026-0253 (`lru`, unsound, no patched release) |
| no release exists | `git tag \| wc -l` | `0` |
| board census | `jq -r '.status' .coding-hermes/board/tasks.jsonl \| sort \| uniq -c` | 37 complete, 4 pending, 41 rows |

The board census in the document is pinned to the date of this audit rather
than stated as a rolling count, so a later append does not make the sentence
false; it is a historical snapshot with the pending rows named by id.

## Scope discipline

No compatibility, drop-in, or performance claim was added, weakened, or
re-scoped. The rewrite states the decisions the project already recorded
(Turso not adopted at 0.8.3; PostgreSQL out of scope until the AC-4 round-trip
is resourced) as decisions, and names the one unproven project-level criterion
the matrix already records: the AC-2/AC-9 paired upstream-harness comparative
run has never been executed.

Docs only: no Rust source, fixture bytes, or pinned upstream material changed.
