# Hermes RS dogfood run 5 — 2026-10-27

## Promise

Hermes RS claims that a user can build the source checkout and use the documented standard-library Python client to drive the Rust SQLite session-store adapter over newline-delimited JSON-RPC, persist and reopen sessions, search/read them, record usage, back up the database, and receive structured errors. It explicitly does not claim to be full Hermes Agent or a published package.

## Target and identity

The scheduler stand-in path resolved to the canonical repository `/home/kara/hermes-rs`; the stand-in itself resolves to `/home/kara`, so no product commands were run from the stand-in. The clean detached dogfood worktree and the bunker install leg both tested commit `9e23afdfb8f4717474e27cc9d80773c01a5084d6`, equal to `origin/main`. The canonical checkout was clean before product use; the board write later introduced only the expected board files and an external worker concurrently modified `events.jsonl`.

## Real-use workflow

From a clean detached worktree, with `CARGO_TARGET_DIR=/tmp/hermes-rs-dogfood-20261027-210136/target`:

```text
cargo build --bins                         rc=0, dev build 9.29 s wall
hermes-rs-session-adapter --protocol       hermes-rs/session-adapter/1
python3 scripts/hrs003_e2e_proof.py --adapter <debug adapter>  rc=0
```

The proof drove the public `scripts/hrs003_client.py` surface, not internal Rust helpers. It created a store, appended messages, used `append_messages_batch`, recorded model usage, read and reopened the same file through a second adapter process, read it with Python stdlib `sqlite3`, checked integrity/schema, exercised a structured missing-session error, and shut down cleanly. Captured output shows `BATCH_WIRE_OK`, `MODEL_USAGE_WIRE_OK`, `GET_SESSION_WIRE_OK message_count=7 == len(get_messages)=7`, `ROUNDTRIP_OK`, `SQLITE3_REOPEN_OK ... integrity=ok`, `SQLITE_SCHEMA_VERSION=31`, `ERROR_STRUCTURED ... AdapterError(SessionRowMissing...)`, and `LIFECYCLE_CLEAN`. The 200-message batch/usage arm also passed. A warm repeat returned the same markers.

## Performance

The headline user workflow is comfortably fast for a one-user local persistence slice; no PERF row is justified. Cold detached build: 9.29 s wall (first build in that isolated target). Cold proof process: 9.10 s wall, 0.24 s user CPU, 0.09 s system CPU. Warm proof repeat: 5.46 s wall, 0.21 s user CPU, 0.08 s system CPU. The proof includes process startup, database creation, backup, and the two-process lifecycle, so it is not a per-query benchmark. The measured waits were not user-noticeable enough to warrant profiling; no flamegraph was made.

## Fresh-machine install leg

The prescribed `bunker-qa.sh` install leg ran on `bunker-mvp`, agent `68817f50`, and tested the same HEAD. Prelaunch status was ONLINE, `bunkerd=active`, disk 32% (47.5 GB/149.9 GB), 1/50 agents; the pre-existing `crier-lab` agent was not touched. `fresh-install` was OK: build-only install, rc=0, dev profile finished in 19.17 s. `ci-pass` was OK: one act job green, with the complete `ci.log` harvested. `chaos-disconnect` was INFO with the network cut applied and suite result `52 0`; the harvested log shows the Rust suite ran and the adapter malformed/unknown-method/round-trip tests passed. `chaos-resource` passed under a 3 GB memory cap. There are no release tags, compose file, or web UI, so upgrade/docker/UI cells were N/A for explicit reasons. The battery's `chaos-corruption` was N/A with `no db/state files in repo`, and `chaos-errorpath` was INFO with `no start command detected / entrypoint not observable`; these are harness coverage gaps for this repo, not product failures, and are filed as DF-HRS-120 and DF-HRS-121.

Collection completed and the agent was destroyed. Post-destroy `bunker info 68817f50` returned `not_found`, and the fresh server census showed only the pre-existing `crier-lab` agent.

## Findings

DF-HRS-120: the bunker corruption detector searches only repository files and therefore misses the runtime-created `state.db` that is the product's main surface. It skipped corruption coverage instead of creating a store through the documented adapter flow.

DF-HRS-121: the bunker error-path detector does not recognize the documented Rust binaries/stdio adapter, leaving missing/invalid-input behavior unverified even though the local proof exercises one structured error and the suite contains malformed/unknown-method negative tests.

## Value verdict

PROMISING-BUT-ROUGH. The implemented persistence slice delivers its documented workflow: the real Python consumer completed round-trip, independent SQLite read-back, batch usage, structured error, and clean lifecycle checks at the tested HEAD. The remaining roughness is dogfood/install observability: the fresh-machine battery cannot discover runtime state or the documented CLI/stdio negative path, so two important user-facing reliability dimensions remain unproven by the install leg. No product source was changed.

## Evidence

All captured transcripts are in `docs/evidence/dogfood-20261027/`: `build.log`, `e2e.log`, `e2e-warm.log`, `bunker-evidence.jsonl`, `chaos-disconnect.log`, and `bunker-ci.log`. The run's rows are DF-HRS-120 and DF-HRS-121. The run was executed directly; no retired dogfood driver was used.
