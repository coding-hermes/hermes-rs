# Release readiness — hermes-rs — 2026-11-04

Sweep id: `hermes-rs-releng-2026-11-04-15-35-32`
Project: `hermes-rs` (scheduler row `hermes-rs-releng`, parent `hermes-rs-foreman`)
Repository: `coding-hermes/hermes-rs` — https://github.com/coding-hermes/hermes-rs
Canonical checkout: `/home/kara/hermes-rs`

## Decision

**NO CUT — NO-GO (Decision 1 rule 2, plus Decision 1b: no release surface).**

Patch/minor authority is standing under the releng charter; this is *not* an
"awaiting owner authorization" hold. The cut is impossible for repo-owned
reasons, and the single owner-scoped question (first-use artifact channel) is
escalated once below with a recorded silence default.

Why, in order:

1. **Open P1 blocker that the repo's own roadmap makes a release gate.**
   `specs/ROADMAP.md` Gate 8: "Gate releases on compatibility evidence,
   clean-machine installation, security review, data migration/rollback, and
   documented support versions." `HRS-127` (P1, pending) records that the
   paired upstream-harness comparative run required by AC-2/AC-9 has **never
   been executed**, and the compatibility matrix row "Upstream test framework
   and comparative scoring" is still `Untriaged`.
2. **No release surface exists.** Zero tags local and remote, zero Release
   objects, no CHANGELOG, no release driver, no tag-triggered workflow, no
   artifact/checksum publication path. A tag cannot be cut into a vacuum and
   could not be verified afterwards.
3. **A tag would certify a state the repo denies.** ROADMAP: "No release tag
   and no published artifact exists; the repository installs from a source
   checkout only," and "Every subsystem other than persistence remains
   'Pending Rust implementation' in the matrix."

## Candidate

| Field | Value |
|---|---|
| Candidate SHA | `f9dfd927a2ea121a3468c63a7674f4fdd4e0dbbf` |
| `git ls-remote origin refs/heads/main` | `f9dfd927a2ea121a3468c63a7674f4fdd4e0dbbf` |
| `gh api repos/.../commits/main` | `f9dfd927a2ea121a3468c63a7674f4fdd4e0dbbf` |
| Local `HEAD` | `f9dfd927a2ea121a3468c63a7674f4fdd4e0dbbf` |
| Parity `git rev-list --left-right --count origin/main...HEAD` | `0  0` |
| Working tree | clean (`git status --porcelain` empty) |

Note on a transient three-way disagreement observed during this sweep: the
first probe pair returned `gh api` = `d09f4a5` while `git ls-remote` =
`f9dfd92`. A sibling lane pushed `f9dfd92` while the sweep was starting. The
re-probe after `git fetch` put all three views (local HEAD, ls-remote,
provider API) on `f9dfd927`, which is the candidate recorded above. The sweep
was re-anchored to the final tip; no evidence below predates the re-anchor.

## Inventory since the last release artifact

Release baseline: **none.** `git tag | wc -l` = 0; `git ls-remote --tags
origin` = empty; `gh api repos/coding-hermes/hermes-rs/tags --jq length` = 0;
`gh api repos/coding-hermes/hermes-rs/releases --jq length` = 0. The public
surface was proven absent on the remote, not only in the checkout.

Full reachable history: **189 commits** (`git rev-list --count HEAD`).

Subject-prefix census:

| prefix | count |
|---|---|
| `board` | 65 |
| `hrsforeman` | 22 |
| `fix` | 16 |
| `hrs_worker` | 9 |
| `docs` | 9 |
| `releng` | 6 |
| `qa` | 6 |
| `dogfood` | 6 |
| `feat` | 5 |
| `test` | 3 |
| `sec` | 2 |
| `chore` | 1 |
| non-conventional | 39 |

Board/harness bookkeeping (`board`, `hrsforeman`, `hrs_worker`, `releng`,
`qa`, `dogfood`) accounts for **114 of 189** commits. Material count is
therefore at most 75, and the 39 non-conventional subjects require individual
classification before any first-release changelog can be written.

Breaking changes: **0** — `grep -c 'BREAKING[ -]CHANGE'` over all commit
bodies = 0; bang-marked subjects (`^[a-z]+(\(...\))?!:`) = 0. Zero across the
whole history is a fact about this repo, not an assumption.

## Version hygiene

| Surface | Value | Status |
|---|---|---|
| `Cargo.toml` `[package] version` | `0.1.0` | present |
| `Cargo.lock` | consistent with manifest | clean |
| `CHANGELOG` | **absent** | DIRTY (gap) |
| `VERSION` file | absent | n/a (Cargo owns the stamp) |
| `AGENTS.md` release policy | absent (no AGENTS.md) | DIRTY (gap) |
| docs naming a version | none | DIRTY (gap) |

A version stamp exists; the changelog, release policy and versioned docs that
Decision 2 requires before tagging do not. `0.1.0` is the crate's initial
manifest version, not a release decision.

## CI truth on the candidate

| Workflow | Run id | SHA | Conclusion |
|---|---|---|---|
| `CI` (project contracts + board integrity) | `41859642830` | `f9dfd92` | success |
| `Rust CI` (fmt, clippy, test) | `41843588109` | `7056810` | success |
| `CI` | `41843588007` | `7056810` | success |

`gh api repos/.../commits/f9dfd92.../check-runs --jq .total_count` = **1**
(the `Project contracts and board integrity` job, success). The Rust CI
workflow is path-filtered on `Cargo.toml`, `Cargo.lock`, `rust-toolchain*`,
`**/*.rs`, `scripts/hrs003_*`. The candidate and every commit after `7056810`
are board/docs-only, so no Rust CI run exists for them **by construction** —
this is a property of the tip, not absent CI and not a defect.

**Tree identity proof for the Rust gate.** `git diff --name-only
7056810..HEAD` returns exactly:

```
.coding-hermes/board/board.jsonl
.coding-hermes/board/events.jsonl
.coding-hermes/board/tasks.jsonl
docs/evidence/hrs126/audit.log
docs/evidence/hrs126/roadmap-current-state.md
specs/ROADMAP.md
```

Zero `.rs`, `Cargo.toml`, or `Cargo.lock` paths. The Rust code tree at the
candidate is byte-identical to the tree run `41843588109` tested, so that
green stands by tree identity rather than by assumption.

There is **no tag-triggered workflow in this repository**: all three workflows
are `on: push (branches: [main])`, `pull_request`, and/or `workflow_dispatch`,
one of them path-filtered. Nothing can run on a tag as things stand.

## Local gate replay at the candidate

Run serially from `/home/kara/hermes-rs` at `f9dfd92`, tree clean:

| Command | rc | Result |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 | clean |
| `cargo test --workspace --all-features` | 0 | **65 passed, 0 failed, 3 ignored** across 18 binaries |

This is local substitute evidence; it does not replace the hosted exact-SHA
gate, which is satisfied separately above.

## Dependency state (Decision 3)

Scanner, not updater: `cargo audit --json` → rc 0.

| Metric | Value |
|---|---|
| vulnerabilities | **0** |
| warnings | 1 (`unsound`) — RUSTSEC-2026-0253, disclosed and closed under `SEC-HRS-001`; no patched release exists |

No advisory affects a shipped runtime dependency at this candidate. No
patch-level batch is due at release time because there is no release. No
dependency major bump is proposed.

## Release tooling inventory

| Probe | Result |
|---|---|
| `Makefile` / `makefile` / `GNUmakefile` | absent |
| `.goreleaser*` | absent |
| `Dockerfile` / `docker*` | absent |
| `.github/workflows/` | `ci.yml`, `release-parity.yml`, `rust-ci.yml` — none publishes anything |
| `scripts/*release*` | `check_release_parity.py`, `release_areas.py`, `track_upstream_release.sh` — all **upstream** release tracking, none is this project's release driver |
| `CHANGELOG*` | absent |
| `AGENTS.md` release policy | absent (no AGENTS.md) |
| release docs | absent |

`release-parity.yml` is an upstream-parity fixture gate; its name is
misleading as a release signal and it publishes no artifact.

**The repository has no release driver and no release documentation.** Under
Bane doctrine a project is expected to ship a release script plus release
docs; this one ships neither. That is a dispatchable gap (it publishes
nothing) and is filed as `RELEASE-001`.

## Deployed staleness

`command -v hermes-rs hermes-rs-session-adapter` → not on PATH.
`/home/kara/.local/bin/hermes-rs*` → absent.

No installed binary exists, so there is no stale deployed copy that could lie
to later verification runs. Install path today is a source checkout only,
consistent with the README and ROADMAP.

## Public posture (unchanged by this sweep; re-measured)

| Item | Measured |
|---|---|
| License file | `LICENSE` present, MIT (`Cargo.toml` `license = "MIT"`) |
| Published releases / tags | 0 / 0 |
| Branch protection on `main` | not protected (carried forward from the 2026-11-01 sweep; owner-scoped decision) |

Branch protection is a standing owner decision and is **not** filed again
here — a second row for the same class splits its closure. It is recorded as
carried forward.

## Board hygiene

Actual board census at this sweep: **44 rows — 38 complete, 6 pending.**

Pending rows:

| id | pri | title (truncated) |
|---|---|---|
| `HRS-127` | P1 | Paired upstream-harness comparative run (AC-2/AC-9) never executed; comparative-scoring matrix row still `Untriaged` |
| `HRS-128` | P1 | Tier-2 cannot certify work: evaluator test-log poll loop wedges when its cargo process disappears |
| `QA-HERMES-RS-FOREMAN-1` | P1 | Battery launch refused: bunker-mvp disk at 98% exceeds 94% preflight threshold |
| `RELEASE-READINESS-2026-11-01` | P1 | Release readiness sweep — NO-GO (this row; refreshed by this sweep) |
| `DF-HRS-123` | P2 | Fresh-machine install leg could not spawn on bunker-mvp |
| `NEVER-DONE` | P3 | Perpetual audit fixture, never closed |

Drift found: `specs/ROADMAP.md:11` states "41 rows: 37 complete and 4 pending"
and lists four pending rows. Actual is 44/38/6 — `HRS-127` and `HRS-128` are
missing from the stated census. The line was corrected today in `d09f4a5` and
was made stale again by `f9dfd92` within the same hour, which makes it a
structurally unstable claim rather than a one-off typo. Filed as `DF-HRS-126`.

## What this sweep did not do

- Did not create a tag, Release object, or any artifact.
- Did not move dependencies.
- Did not touch branch protection.
- Did not repair the board validator's inherited `events.jsonl` descending-id
  condition carried forward from the prior sweep.

## Required next gate

`HRS-127` (paired upstream-harness comparative run) must produce case-level
results, and `RELEASE-001` (release surface: driver, changelog, tag-triggered
CI, checksum manifest, release docs) must land, before any readiness sweep can
return GO. The first-use artifact channel is escalated in `RELEASE-001` with a
recorded silence default.

## Verification addendum — re-delivery of the same sweep id

Written 2026-11-04T15:49Z. The scheduler delivered sweep id
`hermes-rs-releng-2026-11-04-15-35-32` a second time (slot 15:35:32Z = 10:35
local; the first pass committed 8 minutes later). Same id means one instance,
so this pass verified the landed evidence instead of re-running the sweep and
re-cutting. Nothing above was re-derived; every claim below was re-measured at
the tip now being served.

| Claim from the first pass | Re-measured | Result |
|---|---|---|
| Candidate `f9dfd92` is an ancestor of the landed tip | `git merge-base --is-ancestor f9dfd92 HEAD` | holds |
| Tip after the carrier commit | local `HEAD`, `git ls-remote origin refs/heads/main`, `gh api .../commits/main` | all three = `527f8ba7fdedd3d7c1d0e19cff214c6d6bf3c53a` |
| Parity | `git rev-list --left-right --count origin/main...HEAD` | `0  0`, tree clean |
| Rust gate by tree identity | `git diff --name-only f9dfd92..527f8ba` | 3 paths, all `.coding-hermes/` — **0 non-board files**, so the code tree is byte-identical to `7056810` and run `41843588109` stands |
| Exact-tip CI | `gh api .../commits/527f8ba/check-runs` | `total_count` = 1, `Project contracts and board integrity` = success (run `41860800831`) |
| Prior candidate CI | run `41859642830` at `f9dfd92` | success, unchanged |
| Public release surface | `gh api .../tags` and `.../releases` | 0 and 0; `git ls-remote --tags origin` empty |
| Board census | `jq` over `tasks.jsonl` | **46 rows — 38 complete, 8 pending** (delta vs this doc's 44/38/6 is exactly the two rows this sweep itself filed: `RELEASE-001`, `DF-HRS-126`) |
| Board validator | `boardctl validate` | FAIL (1 error, 38 warnings) — unchanged baseline; the error is the inherited `events.jsonl` line 73 descending id |
| Advisory scan | `cargo audit --json` | 0 vulnerabilities, 1 `unsound` warning, `RUSTSEC-2026-0253` (disclosed and closed under `SEC-HRS-001`; no patched release exists) |
| Version surface | `Cargo.toml`, `CHANGELOG*`, `VERSION` | `0.1.0`; no changelog, no VERSION file — gap unchanged |
| Deployed staleness | `command -v hermes-rs*`, `~/.local/bin/hermes-rs*` | absent — no stale installed binary exists |

Decision unchanged: **NO CUT (rule R2 plus Decision 1b).** No tag was created,
no Release object was published, no dependency moved, and no duplicate row was
filed — identical state to the first pass, so this addendum is the only write.
Open gates remain `HRS-127` (paired upstream-harness comparative run) and
`RELEASE-001` (release surface). Both are pending and dispatchable to the
foreman; neither is an owner-authorization hold.

### Carrier repair

The first addendum carrier `c99dea7` failed its exact-tip CI, run
`41862012532`, on the `Check committed whitespace` step (`git show --check`):
this addendum had introduced one blank line at EOF. Every other step of the job
passed (`Validate kickoff contracts and board JSONL`, `Check the usage skill's
board claims`, checkout). The blank line was removed so the file ends with a
single newline, `git diff --check` reports clean, and the repair carrier is
verified by its own exact-SHA run. The failed carrier is recorded rather than
hidden; the decision below is unaffected.
