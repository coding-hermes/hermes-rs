# Hermes RS dogfood run 7 reproof

Tick: hermes-rs-dogfood-2026-10-31-00-08-45
Target: hermes-rs
Published target: origin/main at 0bb6e3110a3cb8cd793feec61c0b506389602e02
Probe checkout: clean archive materialized from origin/main

Promise exercised: a source-checkout user can run the documented hermes-rs CLI and drive the documented stdlib Python session-adapter workflow.

CLI probe:
- `cargo run --quiet --bin hermes-rs` cold: exit 0, 7.90s wall, 20.04s user, 2.91s sys; printed `Hermes RS scaffold: no agent functionality implemented`.
- Same command warm: exit 0, 0.07s wall, 0.05s user, 0.02s sys.
- `cargo run --quiet --bin hermes-rs -- --help`: exit 0, printed only the scaffold line; no usage/help text.
- `cargo run --quiet --bin hermes-rs -- --bogus`: exit 0, printed only the scaffold line; no unknown-argument diagnostic.
- `cargo run --quiet --bin hermes-rs-session-adapter -- --protocol`: exit 0, printed `hermes-rs/session-adapter/1`.
- Malformed adapter frame `not-json`: exit 4, stderr `malformed request frame: expected ident at line 1 column 2`.

Adapter workflow:
- `python3 scripts/hrs003_e2e_proof.py --adapter <isolated debug adapter>`: exit 0, 5.87s wall, 0.22s user, 0.07s sys.
- Batch append, model usage, get-session/message-count parity, two-process reopen, stdlib sqlite3 integrity read-back, schema version 31, structured missing-session error, and clean lifecycle all passed.

Finding: DF-HRS-124. The scaffold CLI accepts both its conventional help request and an unsupported argument as successful no-op invocations. This is a usability/contract gap, not a persistence-engine failure. Evidence is also preserved in `docs/evidence/dogfood-20261031-run7-reproof/` and the board row points to the raw scratch logs.

Performance judgment: the completing CLI operation is 0.07s warm and not user-noticeable; the 7.90s cold number includes first build compilation. The adapter proof completes in 5.87s after the isolated build. No PERF row or profile is justified.

Install: `SKIPPED-install-bunker` remains owned by DF-HRS-123. No fresh-machine claim is made: the prescribed bunker launch reached spawn but bunker-mvp SSH port 22 timed out. Existing evidence: `/home/kara/.hermes/cache/scratch/hermes-rs-dogfood-20261031-install.jsonl`.

No product source or usage-skill code was changed. Board validation was re-run after filing: the pre-existing events.jsonl descending-id error remains; DF-HRS-124 occurs exactly once and the new event is present at the board tail.
