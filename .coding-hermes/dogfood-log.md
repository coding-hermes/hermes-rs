# Dogfood Log — hermes-rs

## 2026-10-14 dogfood tick (run 1)
- HEAD: 86f61d9 (main, pushed, CI green on GitHub, act local green)
- Project stage: spec/baseline only — no Rust code, no Cargo.toml. Promise = the
  upstream-baseline + compatibility-matrix contract set (HRS-001 complete).
- Real-use pass: ran both documented user-facing scripts. capture_baseline.py
  --profile single_session|multi_session: both emit valid hermes-rs/baseline/1
  JSON with correct env metadata (CPU model, kernel, 16 cores) and honest nulls
  + pending_implementation marker; bad profile and missing --out both fail with
  clean argparse errors (rc=2). check-project-artifacts.py: "project artifacts
  valid: 5 board rows, 4 kickoff tasks". Timing: single_session 0.026s cold —
  nothing slow enough for a user to notice; no PERF row justified.
- Cross-checks: pinned upstream tag v0.22.0 tag-object SHA verified live via
  git ls-remote (matches UPSTREAM_BASELINE.md). Pinned checkout
  /tmp/hermes-upstream-pin-hrs001 still present (ephemeral — will vanish; a
  re-pin procedure exists in the baseline doc, so acceptable but noted).
- Install leg: bunker-qa fresh-install OK ('no-standard-install-path') +
  ci-pass OK (act 1 job green). Chaos tail TRUNCATED mid-battery — known
  harness class, filed DF-HRS-103.
- Findings filed: DF-HRS-101 (no install path to prove), DF-HRS-102 (nulls
  lack per-field reasons), DF-HRS-103 (battery truncated).
- Verdict: VALUE-OK for stage. The artifacts do exactly what HRS-001 promised
  and honestly refuse performance claims. Usability good for a spec repo;
  the gap is forward-looking (DF-HRS-101).
