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

### Concurrent sibling landing — outside this carrier's evidence boundary

While this carrier was being prepared, a sibling lane committed `0d256a3`
("release: add the release surface — driver, CHANGELOG, tag-triggered publish
workflow, docs") in the shared checkout: `scripts/release.sh`, `CHANGELOG.md`,
`docs/releases.md`, `.github/workflows/release.yml` and three evidence files,
1451 insertions. At the moment of writing that commit was **unpushed** and had
no CI run, so this carrier stays bound to `c99dea7` and publishes only the
board/evidence delta; it does not verify the release surface.

That changes the shape of the next decision but not this one: `HRS-127` (P1,
the paired upstream-harness comparative run named by ROADMAP Gate 8) is still
open, so Decision 1 rule R2 still blocks a cut. When `0d256a3` (or its rebased
successor) reaches `origin/main` with green exact-SHA CI and `RELEASE-001` is
closed, the next sweep re-decides under rules R4/R5 against the new surface —
including executing the new `scripts/release.sh` in a scratch clone before
trusting it, per the driver-execution rule.

## Addendum — second delivery of the same tick id, re-decided at the served tip

Tick id `hermes-rs-releng-2026-11-04-15-35-32` was delivered again. Same id
means one tick: this section is a verification addendum to the first pass, not
a second sweep. The first pass closed with an explicit instruction, and this
pass discharges it:

> When `0d256a3` (or its rebased successor) reaches `origin/main` with green
> exact-SHA CI and `RELEASE-001` is closed, the next sweep re-decides under
> rules R4/R5 against the new surface — including executing the new
> `scripts/release.sh` in a scratch clone before trusting it.

### What changed since the first pass

| Fact | First pass (`f9dfd92`) | This pass (`c888bd8`) |
|---|---|---|
| Release driver / CHANGELOG / publish workflow / docs | absent | present (`0d256a3`, `0d1c301`) |
| `RELEASE-001` | pending P1 | complete, bound to `0d256a3` |
| `scripts/release.sh` executed anywhere | no | dry run rc=0 (this pass, below) |
| aarch64 release build | never attempted | rc=0 (this pass, below) |
| `release.yml` runs ever | 0 | 0 |

The tip moved twice while this pass measured it, both times by a sibling
foreman and both times board-only: `4a8a648` (closed `RELEASE-001`) and
`c888bd8` (recorded the `RELEASE-001` memory receipt as
`namespace-unverified`). The pass opened at `ca3184e` and closed at
`c888bd8`; `git diff --name-only ca3184e..c888bd8` lists only
`.coding-hermes/board/{board,events,tasks}.jsonl`, so every build input the
gates below measured is byte-identical to the candidate they ran on.

### Candidate

| Field | Value |
|---|---|
| Candidate SHA | `c888bd8fbd867b2412ae01dca2c612d29bf81e7c` |
| `git ls-remote origin refs/heads/main` | `c888bd8fbd867b2412ae01dca2c612d29bf81e7c` |
| Provider API `commits/main` | agrees (CI run `41869553562` is bound to this SHA) |
| Local `HEAD` | `c888bd8fbd867b2412ae01dca2c612d29bf81e7c` |
| Parity `git rev-list --left-right --count origin/main...HEAD` | `0  0` |
| Working tree | clean |

Exact-SHA hosted CI on the candidate: run `41869553562` (`CI`) — **success**.
The two intermediate tips are green as well: `41869113771` at `4a8a648`,
`41866937437` at `ca3184e`. `Rust CI` last executed as run `41843588109` at
`7056810`; nothing since touches `.rs`, `Cargo.toml`, `Cargo.lock` or
`rust-toolchain*`, so it stands by tree identity.

### Driver execution (the rule this pass owed)

Clean detached worktree at the candidate; every command run there, none in the
shared checkout.

| Probe | Command | Result |
|---|---|---|
| Plan resolution | `scripts/release.sh --dry-run` | **rc=0**, plan fully resolved |
| Refusal path | `scripts/release.sh` (bare) | **rc=2**, explicit message, no tag created |
| Tree after both | `git status --porcelain`, `git tag \| wc -l` | empty, `0` |

The dry run resolved: version `0.1.0` in `Cargo.toml` matching `Cargo.lock`
`0.1.0`; tag `v0.1.0` absent locally and on origin; no dated `0.1.0` CHANGELOG
section yet; `[Unreleased]` non-empty (58 lines, **16 feature bullets, 0
`chore`/`board` bullets**, so the notes do not lead with bookkeeping churn);
artifact names `hermes-rs-v0.1.0-x86_64-unknown-linux-gnu` and
`hermes-rs-session-adapter-v0.1.0-x86_64-unknown-linux-gnu`; a two-line
`SHA256SUMS` plan. The CHANGELOG transform was exercised into a temporary
directory and discarded — the repository was not written to.

The refusal path at this tip surfaces the main-branch precondition first
(`REFUSED: HEAD is on 'detached HEAD', not main — releases are cut from main`,
rc=2), which is the operational consequence recorded in the decision below:
the real cut must run where `HEAD` is literally `main`.

The surface files (`scripts/release.sh`, `CHANGELOG.md`,
`.github/workflows/release.yml`, `docs/releases.md`) are byte-identical to
`0d1c301`, the commit that carries the tier-2 PASS verdict
(`.gitreins/history/2026-11-04/31f6d666/verdict.json`, RELEASE-001, 6/6 items,
`[DEGRADED — skipped: lint/tests: no staged files]` because the judged commit
was board/docs-only). `git diff --name-only 0d1c301..HEAD -- scripts/
.github/workflows/release.yml CHANGELOG.md docs/releases.md` is empty, so that
verdict carries to this tip by tree identity rather than by re-reading it as
fresh.

### Local gate replay at the candidate

| Gate | Command | rc |
|---|---|---|
| Format | `cargo fmt --all -- --check` | 0 |
| Lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 |
| Tests | `cargo test --workspace --all-features` | 0 — **65 passed, 0 failed, 3 ignored** across 18 test binaries |
| Project contracts | `python3 scripts/check-project-artifacts.py` | 0 |
| Skill-claim check | `python3 scripts/check_skill_claims.py` | 0 |
| Advisory scanner | `cargo audit --json` | 0 — `vulnerabilities.found=false`, 0 advisories, 1 `unsound` warning (`RUSTSEC-2026-0253`, disclosed under closed `SEC-HRS-001`) |

### Artifact-strategy probe: the aarch64 leg now has measured evidence

`release.yml` builds both binaries for `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu`. That workflow has **never run** (0 runs total),
so before this pass the aarch64 leg was unproven anywhere. It was exercised
locally at the candidate with the workflow's own environment:

```
rustup target add aarch64-unknown-linux-gnu                       rc=0
CARGO_TARGET_DIR=<scratch>/arm64-target \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
cargo build --release --locked --target aarch64-unknown-linux-gnu \
  --bin hermes-rs --bin hermes-rs-session-adapter                  rc=0
```

Products: `hermes-rs` 1,012,480 bytes and `hermes-rs-session-adapter`
3,137,112 bytes, both `ELF 64-bit LSB pie executable, ARM aarch64`, built in
1m11s. Bundled SQLite cross-compiled without an extra C-toolchain override, so
the publish leg's risk narrows to the workflow wiring (prepare checks, artifact
naming, `SHA256SUMS` assembly, release creation), not the cross-compile. These
are strategy evidence, not publishable assets, and nothing was published.

### Decision — unchanged in verdict, reduced in reasons: NO CUT

The rule that fires is Decision 1 rule 2 as the repository itself writes it,
plus Decision 1b. `specs/ROADMAP.md` Gate 8 states it plainly:

> Gate releases on compatibility evidence, clean-machine installation, security
> review, data migration/rollback, and documented support versions.

and the same document's finish line adds that "a supported release must run
the upstream Hermes harness against the Python reference and Rust target
through an explicit adapter... and ship tested install, upgrade, backup,
migration, and rollback paths." At this candidate:

1. `HRS-127` (P1, pending) still records that the paired AC-2/AC-9
   comparative run has never been executed and that the compatibility matrix's
   comparative-scoring row is still `Untriaged`.
2. `DF-HRS-123` (P2, pending) still owns the unproven clean-machine install
   leg; `QA-HERMES-RS-FOREMAN-1` (P1) records the battery launch refused on
   host capacity, so the QA leg carries no fresh evidence either.
3. No install, upgrade, backup, migration or rollback path has been exercised
   for a released artifact — there has never been one.

What is retired from the first pass's NO-GO: **"no release surface exists" is
no longer true**. The surface exists, is executable (dry run rc=0), refuses
safely (bare rc=2), and its publish driver has never run. A v0.1.0 tag today
would still certify a product state the repository's own roadmap denies, so
the cut stays refused — for one fewer reason, with the gate named.

One new dispatchable gap was filed: **`RELEASE-002`** — the publish path has
never executed, so the first tag push would be its first run, at a moment when
the tag is already published and immutable. Its acceptance criteria ask for a
build-only dispatchable path plus one green run that produces all four
artifacts and verifies `SHA256SUMS`; publishing stays gated behind a real tag.

### Operational note for the executing pass

`scripts/release.sh` refuses unless `HEAD` is on a branch literally named
`main`. A detached worktree (how every gate in this lane is measured) can run
`--dry-run` but cannot run the cut. The authorized pass therefore runs the cut
either in the canonical checkout while it is clean and quiescent, or in a fresh
`git clone` of the published remote where `main` is checked out — and it must
re-check `git log -1` immediately before, because this repository's board is
being written by sibling lanes every few minutes.

### Dependency state (Decision 3)

No advisory: `cargo audit` reports 0 vulnerabilities. An updater listing
(`cargo update --dry-run`) shows **42 packages** with compatible updates
available, all patch/minor: `tokio 1.53.3 → 1.54.1`, `libc 0.2.190 → 0.2.191`,
`rustix 1.1.5 → 1.1.6`, `smallvec 1.16.3 → 1.16.4`, `uuid 1.28.0 → 1.29.0`,
`cc 1.8.0 → 1.11.0`, `icu_* 2.3 → 2.4` (9 crates), `log 0.4.34 → 0.4.35`,
plus transitive patch bumps. Under Decision 3 patch-level moves batch at
release time, and the release is refused, so nothing was moved. No major bump
is pending. `Cargo.lock` is unchanged by this pass.

### Public posture (re-measured)

| Item | Reading |
|---|---|
| Visibility / license | public, `MIT` (`LICENSE` present) |
| Secret scanning | **disabled** |
| Push protection | **disabled** |
| Validity checks / non-provider patterns | disabled |
| Dependabot security updates | disabled |
| Branch protection | HTTP 404 `Branch not protected` — owner decision, not touched |
| Open secret-scanning alerts | query returns 404 because the scanner is off; the "0" this would print proves nothing |

The disabled-scanner rows are recorded, not repaired: enabling them is an
owner-scoped repository setting, and branch protection would block every lane's
direct push to `main`.

### Board state

46 rows: 39 complete, 7 pending (`NEVER-DONE`, `DF-HRS-123`, `HRS-127`,
`HRS-128`, `DF-HRS-126`, `QA-HERMES-RS-FOREMAN-1`,
`RELEASE-READINESS-2026-11-01`). `boardctl validate` baseline is unchanged at
1 error / 38 warnings, the error being the pre-existing `events.jsonl` line 73
descending-id defect (id 73 below 396) — inherited, not introduced here.

### Rollback

Nothing was created: 0 tags local, 0 tags on origin, 0 Release objects, no
artifact published, `Cargo.lock` and every source file untouched. There is
nothing to roll back. `git tag | wc -l` = 0 after this pass as before it.
