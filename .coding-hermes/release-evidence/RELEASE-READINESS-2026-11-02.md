# hermes-rs release readiness — 2026-11-02

Decision: NO-GO / NO-CUT. This scheduler tick is a readiness sweep, not release authorization. No tag, GitHub Release, or publication mutation was performed.

Target and evidence boundary
- Repository: coding-hermes/hermes-rs
- Canonical remote: git@github.com:coding-hermes/hermes-rs.git
- Selected branch: main
- Tested readiness candidate: 7bcec2538cad646ae7b69ee8ea601ea2e334a71d
- Evidence-only carrier: 25549e131ce77fdf3d631bb1a3b06cd6bb91928f; its exact-head CI run 41374481971 completed success
- Remote main and local HEAD equal the carrier; the carrier contains only the readiness artifact and board evidence update
- The detailed local gates and change inventory below are bound to the tested readiness candidate, not silently promoted to the carrier
- Existing local and remote release tags: none; no prior Release objects
- Version surface: Cargo.toml package version 0.1.0; no CHANGELOG or numeric changelog heading
- Semver: first-release proposal only. v0.1.0 is a proposal, not authorization.

Change inventory
- Full reachable history: 172 commits; no since-tag range exists
- Conventional subjects: feat 5, fix 16, perf 0, other 151
- BREAKING CHANGE footers and breaking subject markers: 0
- Change mix does not authorize a first release; explicit owner authorization and release contract are required.

CI exact-head evidence
- GitHub run 41374287052: CI, push, completed/success, candidate SHA exact; https://github.com/coding-hermes/hermes-rs/actions/runs/41374287052
- The prior candidate run 41366313262 was also completed/success at 15156f7; the current candidate has its own green run.
- Tag-triggered CI: UNVERIFIED / structural blocker. Existing workflows have no tag push trigger; no test tag was created.

Local candidate gates
- cargo fmt --all -- --check: PASS
- cargo clippy --workspace --all-targets --all-features -- -D warnings: PASS
- cargo test --workspace --all-features: PASS; 64 passed, 0 failed, 1 ignored
- cargo build --bins --release --locked: PASS; hermes-rs and hermes-rs-session-adapter produced
- scripts/check-project-artifacts.py: PASS; 38 board rows, 4 kickoff tasks
- scripts/check_skill_claims.py: PASS; 0 violations
- GitReins 0.17.1 guard --full --scope working-tree --json: PASS; degraded=false, skippedSteps=[], changedFileCount=0. Bounded limitation: gitleaks did not run because the candidate worktree had no files in scope.
- The post-board-commit hook was a degraded diff-mode bookkeeping guard: lint/tests skipped because the only staged change was board JSONL. It is not substituted for the fresh candidate full guard above.

Release surface and artifact readiness
- Repository-owned release tooling: NOT FOUND. No Makefile release target, release script, GoReleaser, Dockerfile/registry path, npm package, or Python package.
- Existing release-related scripts and release-parity workflow track upstream parity; they do not build or publish hermes-rs artifacts.
- Cargo release binaries were built for readiness only. No semver tag/provenance stamp exists to verify.
- README states there is no published package or installable artifact; consumers use a source checkout. No deployed hermes-rs binary was found in checked PATH locations; deployed freshness is N/A.
- Artifact publication: UNVERIFIED / no repository-owned packaging or publication contract.

GitReins and dogfood
- Fresh candidate guard: PASS as above; prior receipts are task-bound and not substituted for this candidate.
- Latest dogfood remains PROMISING-BUT-ROUGH. The mandatory fresh-machine install leg remains SKIPPED-install-bunker under DF-HRS-123 because bunker-mvp SSH port 22 timed out. DF-HRS-123 remains an open pending board row.
- The board validator reports one inherited events.jsonl ordering error and 35 warnings; this sweep did not repair unrelated historical board topology.

Public posture and blockers
- GitHub repository visibility: public; Cargo.toml declares MIT.
- Branch protection: NOT CONFIGURED; GitHub API returned 404, "Branch not protected".
- Blocking findings: no explicit first-release authorization/version; no release tooling or publishable artifact contract; no tag-triggered CI; open DF-HRS-123 fresh-machine evidence gap; public main is unprotected.

Unblock path
1. Owner explicitly authorizes a named first-release version and release cut.
2. Establish repository-owned release tooling and docs, including version/changelog, artifact packaging, checksums, and publication rules for both binaries.
3. Add and verify tag-triggered CI on the exact tag commit.
4. Resolve or explicitly owner-waive DF-HRS-123.
5. Add branch protection or record an explicit owner decision accepting the public-repository posture finding.

Evidence sources
- Candidate and version/change inventory: Git history and Cargo.toml at 7bcec2538cad646ae7b69ee8ea601ea2e334a71d
- CI: GitHub run 41374287052, exact candidate SHA
- Release surfaces: .github/workflows/, Cargo.toml, README.md, scripts/release_areas.py, scripts/check_release_parity.py
- GitReins: candidate-bound JSON receipt from gitreins 0.17.1
- Dogfood: .coding-hermes/dogfood-log.md, docs/dogfood/2026-10-31-run7-reproof.md, pending DF-HRS-123
