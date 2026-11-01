# hermes-rs release readiness — 2026-11-01

Decision: NO-GO / NO-CUT. This is an unauthorized readiness sweep, not a release authorization. No numeric release tag exists, the repository has no repository-owned release script or artifact publication path, tag-triggered CI is absent, and dogfood still has an open fresh-machine install blocker.

Target
- Repository: coding-hermes/hermes-rs
- Canonical remote: git@github.com:coding-hermes/hermes-rs.git
- Candidate: 24ebb24f1b131e21bddb3aab0394c3a939383d32
- Candidate source: origin/main; local HEAD and origin/main were equal (0 ahead / 0 behind)
- GitHub branch API SHA: 24ebb24f1b131e21bddb3aab0394c3a939383d32
- Remote branch SHA: 24ebb24f1b131e21bddb3aab0394c3a939383d32
- Release baseline: no tags exist; full-history inventory only, not a since-tag range

Change inventory
- Reachable commits: 169
- feat: 5
- fix: 16
- perf: 0
- other (board/chore/docs/qa/etc.): 148
- BREAKING CHANGE footers: 0
- Semver: first-release proposal only. v0.1.0 may be proposed; no bump is authorized or selected from the untagged full history.

CI
- Exact candidate run 41352067747: completed / success
- Job: Project contracts and board integrity, success
- URL: https://github.com/coding-hermes/hermes-rs/actions/runs/41352067747
- Recent run list: the 20 listed runs were completed/success, including the exact candidate run.
- Workflows: CI, Release parity gate, Rust CI
- Tag CI: UNVERIFIED / structural blocker. Workflows trigger on main pushes, pull requests, workflow dispatch, or selected main-path changes; none has a tag push trigger. No tag was created to manufacture evidence.

Local release gates from a clean detached worktree at the candidate
- cargo fmt --all -- --check: PASS
- cargo clippy --workspace --all-targets --all-features -- -D warnings: PASS
- cargo test --workspace --all-features: PASS, 64 passed, 0 failed, 1 ignored
- cargo build --bins --release --locked: PASS; built hermes-rs and hermes-rs-session-adapter
- python3 scripts/check-project-artifacts.py: PASS; 37 board rows, 4 kickoff tasks
- python3 scripts/check_skill_claims.py: PASS; 0 violations
- GitReins 0.17.1 guard --full --scope working-tree --json: PASS, degraded=false, skippedSteps=[], changedFileCount=0. Bounded receipt: gitleaks did not run because working-tree scope had no files to scan.

Release surface and version hygiene
- Cargo package version: 0.1.0
- License: MIT
- No CHANGELOG file or numeric changelog heading found.
- No Makefile release target, release script, GoReleaser configuration, Dockerfile/registry publication path, npm package, or Python package found.
- Existing release-related tooling is upstream release-parity tracking only: scripts/track_upstream_release.sh, scripts/check_release_parity.py, scripts/release_areas.py, and .github/workflows/release-parity.yml. It does not build or publish hermes-rs artifacts.
- README explicitly says there is no published package or installable artifact yet; use a source checkout.
- No hermes-rs or hermes-rs-session-adapter installed binary was found in the checked PATH locations; deployed-binary freshness is N/A, with no deployed artifact to verify.

GitReins and dogfood
- Recent repository verdicts include PASS results for DF-HRS-120, DF-HRS-121, and DF-HRS-124, but older receipts are task-bound and several are degraded because staged-file checks were skipped; they are not a substitute for the fresh candidate guard.
- Latest dogfood run 7 remains PROMISING-BUT-ROUGH. Adapter workflow and CLI contract were re-proven, but the mandatory bunker fresh-install leg is SKIPPED-install-bunker under DF-HRS-123 because bunker-mvp SSH port 22 timed out. The pending board row remains open.
- Board baseline also has an inherited validation error on events.jsonl line 73 (event id 73 descends below earlier id 396) and 34 warnings; this sweep did not repair unrelated historical board topology.

Public posture
- GitHub visibility: public
- License: MIT License
- Branch protection: not configured (GitHub API returned HTTP 404, Branch not protected)
- Tracked endpoint review: no release publication endpoint was identified in the release surfaces inspected.

Unblock path
1. Owner explicitly authorizes a named first release version and release cut; this readiness row is not authorization.
2. Establish repository-owned release tooling and docs, including a version/changelog contract and an executable artifact strategy for both binaries.
3. Add and verify tag-triggered CI on the exact tag commit, then re-run the release sweep after the candidate is re-anchored.
4. Resolve or explicitly owner-waive the open DF-HRS-123 fresh-machine install evidence gap.
5. Add branch protection or record an explicit owner decision accepting the public-repository posture finding.

Evidence sources
- Candidate and board: git history and .coding-hermes/board/tasks.jsonl at candidate 24ebb24f1b131e21bddb3aab0394c3a939383d32
- CI: GitHub run 41352067747, exact candidate SHA
- Local gate logs: /home/kara/.hermes/cache/scratch/hermes-rs-releng-20261101-2312-logs/
- GitReins JSON: /home/kara/.hermes/cache/scratch/hermes-rs-releng-20261101-2312-gitreins.json
- Dogfood: .coding-hermes/dogfood-log.md and pending board row DF-HRS-123
