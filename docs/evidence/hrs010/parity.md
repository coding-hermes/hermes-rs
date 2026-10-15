# HRS-010 parity report — session-store vertical slice vs the pinned upstream reference

| Field | Value |
|---|---|
| Task | HRS-010 (Roadmap Gate 5) |
| Date | 2026-10-15 |
| Upstream pin | v0.22.0 @ `e06df9c2eae6901f4c88f464e0ae6e753c0428b8` (harness SHA; same pin as every HRS-001/002 row) |
| hermes-rs HEAD at authoring | `4a25415fd96825f241e4bde70367e24490417b40` (branch `wt/HRS-010`, worktree) |
| Scope claim | "the pinned session-store operations named in persistence-shim.md §6.1 behave as specified on stock SQLite" — **no full-Hermes-parity claim** (§6.3(4)) |

## 1. What ran, and through which boundary

The slice was exercised through the HRS-007 fail-closed adapter discipline:
no Python code substitutes for the Rust target; absence/crash is FAIL, never
a pass; every skip carries a reason.

- **Rust arm**: `tests/hrs010_session_store.rs` (22 integration tests) +
  library unit tests, run by `cargo test --workspace --all-features`.
- **Python reference arm** (fixture production + ground truth): the pinned
  checkout's own `SessionDB` at
  `/tmp/hermes-upstream-pin-hrs001` (read-only), via
  `scripts/make_hrs010_fixture.py`. The fixture is Python-produced in the
  strict sense: every row/index/trigger/FTS entry was written by the pinned
  reference's own code paths.
- **sqlite3 arm** (stock-file compatibility, third implementation):
  `scripts/check_hrs010_fixture.sh` + in-test `sqlite3` invocations
  (system sqlite3, a different SQLite build than the one Rust links).

## 2. Adapter-visible persistence cases (denominator)

The classified manifest (`specs/TEST_CLASSIFICATION_MANIFEST.json`, 91
cases: 57 observable / 21 python-internal / 13 platform-host) is scoped to
`tests/providers/` + `tests/conformance/`. The persistence surface of that
denominator is the 13 `platform-host` cases (upstream conformance cells
1/2/3/5). This slice pairs the persistence-relevant cases at the RESULT
level per TEST_ADAPTER_CONTRACT.md §4.3/§8:

| # | Upstream case ID | Class | Python arm (pinned reference) | Rust arm (this slice) | Status |
|---|---|---|---|---|---|
| 1 | `test_cell1_prefix_durability.py::test_acknowledged_appends_survive_sigkill[None]` | platform-host | 89-pass scoped suite run (HRS-005, `baseline/hrs005-scoped-suite.log`) — this case passed in-process | `f3_slice_scale_acknowledged_writes_survive_close_and_reopen` — 25 appends, clean close, contiguous prefix after reopen. **Real-SIGKILL-to-a-child-process leg NOT run** | PAIRED-RESULT (partial; skip reason below) |
| 2 | `..._sigkill[DELETE]` | platform-host | same suite leg | journal resolved `delete` on the pinned interpreter; Rust arm covers the rollback-journal surface via fixture open + `journal_mode_at_rest: delete` in the manifest. **SIGKILL leg not run** | PAIRED-RESULT (partial; same skip reason) |
| 3 | `..._sigkill[WAL]` | platform-host | upstream SKIPS via resolver downgrade gates (pinned interpreter's linked SQLite 3.50.4 is in the WAL-reset bug range) | Rust-side: `f5_passive_checkpoint_policy_and_wal_hygiene` covers the WAL-available leg (bundled SQLite 3.53.2 resolves WAL with verified read-back) | PAIRED-RESULT (upstream skip recorded, not a pass) |
| 4 | `test_cell2_consume_once.py::test_exactly_one_claimant_wins` | platform-host | passed in the scoped suite | **Not implemented in this slice** — `compression_locks`/`session_turn_leases` predicated-UPDATE claiming is out of the §6.1 operation set (deferred; guards exist as typed errors only) | NOT-IN-SLICE (reason: §6.1 scope; lease-claim machinery is a later gate item) |
| 5–7 | `test_cell3_rotation_atomicity.py::test_rotation_is_atomic_under_sigkill[None|DELETE|WAL]` | platform-host | passed in the scoped suite | **Not implemented** — compression rotation (parent end-stamp + child publication in one txn) is out of §6.1 | NOT-IN-SLICE (reason: §6.1 scope; `CompressionClosed` guard tested instead) |
| 8–13 | `test_cell5_delivery_outbox_exactly_once.py` (6 cases) | platform-host | passed in the scoped suite | **Not implemented** — async_delegations delivery outbox is out of §6.1 | NOT-IN-SLICE (reason: §6.1 scope) |

Denominator accounting: 13 platform-host persistence cases →
3 PAIRED-RESULT (each with named partial legs) + 10 NOT-IN-SLICE (each with
the reason above). Zero silent exclusions; no case was counted as passing
without a Rust-side observation point.

The 57 `observable` and 21 `python-internal` cases are transport/registry/
import-order surfaces — outside HRS-010's persistence scope by
classification; the HRS-007 prototype already pairs the 12 observable
transport vectors (`prototypes/test_adapter/last_paired_run_ok.json`).

## 3. Skip / limitation register (fail-closed, with reasons)

| Item | Reason | Recorded where |
|---|---|---|
| Real SIGKILL crash-window leg (F-3 platform-host class) | needs a separate child OS process + barrier discipline; slice proves close/reopen prefix durability in-process. The kill-window harness is a later task. | §2 rows 1–3; test doc comment |
| CJK FTS tokenizer (`cjk_unicode61`) | tokenizer is a loadable extension not present in the bundled SQLite; capability reports `Degraded { reason: "no such tokenizer: cjk_unicode61" }` — never a silent partial search. Fixture records `messages_fts_cjk: unavailable` from the pinned interpreter. | manifest `fts_counts`; capability test |
| WAL on the pinned interpreter | linked SQLite 3.50.4 in the WAL-reset bug range → upstream's own guard resolves `delete` (fixture `journal_mode_at_rest: delete`). Rust's bundled 3.53.2 is NOT in that range and resolves WAL with verified read-back. Both facts recorded. | manifest; generator stderr; capability test |
| `fts_storage_version` marker absent on the fixture | the marker is stamped by the reference only during a v23+ ensure on FRESH stores; the fixture file was produced by the reference's normal open path. Rust stamps it only on fresh scaffolds (same rule). | OpenReport semantics in code comment |
| `advisory_generation_guard = false` | upstream's OFD-lock WAL-generation lockguard surface is not ported in this slice; the capability reports the gap rather than claiming it. | Capabilities struct |
| Display-identity minting (Python `repr`-tuple SHA256) | the durable `display_identity` bytes are computed by the reference from Python tuple reprs; the slice does not mint them (Python recompute paths heal display identities). Documented at the INSERT site. | `stock_sqlite.rs` comment above `INSERT_MESSAGE_SQL` |
| Batch append (`append_messages_batch`) as a distinct txn shape | a batch is expressible as multiple `append_message` calls inside ONE `write()` closure (same txn by construction, §3.3); a dedicated batching API is deferred. | §6.1 mapping note below |

## 4. Fixture parity (Python-produced → Rust → sqlite3)

Producer: pinned reference `SessionDB` (v0.22.0 @ e06df9c2), CPython
3.14.5, SQLite 3.50.4. Artifact: `tests/fixtures/hrs010/state.db` +
`manifest.json` (sha256 `15c91017e358544973a89f11349f9614b591cf4cc1f2e3fbd8378a1067b84c4f`).

| Check | Python reference | Rust store | system sqlite3 |
|---|---|---|---|
| schema_version | 31 | 31 (recognized, F-1) | 31 (`check_hrs010_fixture.sh` step 3) |
| messages rows | 6 (6 live) | 6 read back, exact contents/uids/timestamps (F-2) | 6 + first-content/uid per session (step 4) |
| messages_fts | 6 indexed | search finds `unique_token_xyz` → rowid 5 with BM25 rank + snippet | MATCH → [5] (step 5) |
| messages_fts_trigram | 5 (tool row excluded by view) | table readable (F-1 leg) | 5 (step 4) |
| conversation_generations | 1 for (cli, tg:1003310984808:2) | read + bumped correctly per (source, session_key) (F-9) | verbatim read-back (step 6) |
| session_model_usage | 1 row (accumulated deltas) | upsert/accumulate semantics tested (slice test) | count matches manifest (step 7) |
| integrity_check | ok at production | read-only open leaves file byte-identical (F-1) | ok (step 1) |
| Rust write → third reader | — | Rust writes, closes; system sqlite3 reads the row back, integrity ok, no engine-private objects (F-2/F-10) | verified |

## 5. Build/test commands (acceptance criterion f)

```
# Repo gate (gitreins guard runs the same):
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

# Fixture (regenerate only on a re-pin; requires the pinned checkout):
HERMES_RS_UPSTREAM_PIN=/tmp/hermes-upstream-pin-hrs001 \
  /tmp/hermes-upstream-pin-hrs001/.venv/bin/python scripts/make_hrs010_fixture.py

# Fixture vs manifest via system sqlite3:
./scripts/check_hrs010_fixture.sh

# DDL regen (verbatim upstream DDL → src/persistence/upstream_ddl.rs):
/tmp/hermes-upstream-pin-hrs001/.venv/bin/python scripts/gen_upstream_ddl.py
```

Note: this host exports `CARGO_TARGET_DIR=/var/tmp/hermes-cargo-target`
shared across concurrent builds; a private `CARGO_TARGET_DIR` avoids
file-lock contention with sibling workers. The gate commands are
target-dir-agnostic.

## 6. Non-vacuity evidence

`docs/evidence/hrs010/nonvacuity-mutation.log` — end_session was mutated
to hard-code `end_reason = 'broken-mutation-injection'`; the strengthened
F-9 test FAILED against the mutant and passes against the restored code
(restore verified byte-identical, sha256 f57ae927…).

## 7. Artefact hashes (authoring time)

| Artefact | sha256 |
|---|---|
| tests/fixtures/hrs010/state.db | 15c91017e358544973a89f11349f9614b591cf4cc1f2e3fbd8378a1067b84c4f |
| src/persistence/stock_sqlite.rs | f57ae927a8d065bb4d5ab56678c64fb0bdda6cc5ca02ad318ecd184227792650 |
| tests/hrs010_session_store.rs | 3be55445649d3acf26b43b1abb44a43734269343c971ac76624ed4850a8b04de (pre-format; committed file governs) |
| scripts/make_hrs010_fixture.py | 7c6dbca14c67ec96cf0ebc61bb7f28f8e3526064a9543ac1e5f7c7a8b5e90566 |
| scripts/check_hrs010_fixture.sh | 7954faa10af373fde92425fcf585833c28915026328486b694525b854980d77d |
| scripts/gen_upstream_ddl.py | 70fb54a358f6facc79a4bff495e423f88a1164daffd7058cbcf0138b9f792343 |
| prototypes/test_adapter/adapter.py | 8e4518781b9f551174e514d2b40b2557921e47f8047935cb697e619c40a7eb7b |
| prototypes/test_adapter/corpus.py | 9ecc3c04c6ee62685fbb4e34d749e907d480e13a6a135e5b98406864b3719c90 |

## 8. What this slice does NOT claim

No speed or efficiency statement of any kind (HRS-009 deferred). No Turso/
PostgreSQL statement. No default-backend change. No full Hermes parity:
compression rotation, lease claiming, outbox delivery, display-generation
dedupe, backup-restore CLI flows, and the crash-window harness remain
unimplemented and are enumerated above.
