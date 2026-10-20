# Evidence — dogfood run 3 (2026-10-20), hermes-rs @ `7331204`

Consumer crate written OUTSIDE the repo (`/home/kara/hrs-dogfood-20261020/`); nothing in
this repo was modified to produce these artifacts. Re-run the whole set with
`run_evidence.sh.txt` from that directory.

## Logs

| file | what it is |
|---|---|
| `01-cold-build.log` | `cargo build --release` of the consumer in an empty target dir → 83 s |
| `02-import-both-modes.log` | 4,000 messages imported 3× per mode (`append_messages_batch` vs `append_message` in a loop, one txn per session) |
| `04-verify-stores.log` | system-sqlite3 read-back: integrity, schema 31, counts, uid/timestamp shape, 4,000/4,000 content byte-compare |
| `06-search-fidelity.log` | 20 search probes scored against the corpus-derived token oracle |
| `07-open-cold-warm.log` | cold (fresh scaffold) vs warm (4,000-message store) open timings |
| `08-usage-readback.log` | `record_model_usage` delta folded into the session projection; rollup row visible only via raw SQL |
| `09-backup.log` | backup to the same destination twice (DF-HRS-108 re-proof) |
| `10-adapter-create-typing.log` | adapter `open` with `create`/`read_only` as bool / string / number / null |
| `11-cross-surface-read.log` | the Rust-written store read back through the stdio adapter |
| `12-shutdown.log` | `shutdown` lifecycle: no reply frame, exit 0 |
| `13-search-timings.log` | `hyperfine`, 10 runs per probe, end-to-end CLI search |

## Result sets

- `verify-batch.json`, `verify-single.json` — machine-readable verification of the two stores
- `search-probe.json` — per-probe expected/got/recall/extra + the oracle's own expectations

## Consumer + probe sources (shipped as `.txt`: they are re-runnable outside the repo, not repo code)

- `hrspack-main.rs.txt`, `hrspack-Cargo.toml.txt` — the consumer crate (documented API only)
- `gen_corpus.py.txt` — corpus + oracle generator (200 sessions × 20 messages, seeded)
- `verify_store.py.txt` — independent system-sqlite3 verification
- `probe_search.py.txt` — search-fidelity harness
- `probe_adapter.py.txt`, `probe_adapter_read.py.txt`, `probe_shutdown.py.txt` — adapter probes
- `scan_events.py.txt` — fused/unparseable JSONL census of the committed board events file
- `run_evidence.sh.txt` — regenerates every log above

## Install leg

Launched with `bunker-qa.sh launch /home/kara/hermes-rs --keep` on `bunker-mvp`
(agent `3299f0f7`); evidence path
`/tmp/bunker-qa-evidence-20261020T152815Z-1151160.jsonl` (controller-side, not copied here:
the battery was still running when the run closed, and the agent's 4 h TTL keeps it
recoverable). The run record in `docs/dogfood/2026-10-20-run3-integration.md` §5 carries the
live peek (6 graded cells, a live `cargo`, host disk 100%) and the exact collect command.
