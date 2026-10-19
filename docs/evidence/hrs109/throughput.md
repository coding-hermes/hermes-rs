DF-HRS-109/112 throughput evidence

## In-process smoke (kept, scoped)

Measured on this host by `cargo test --test hrs109_throughput_note -- --nocapture` using fresh stock-SQLite state, the same 50 messages, and one `store.write` transaction per arm. Batch arm: append_messages_batch(50). Comparison arm: 50 calls to append_message in one write closure. This isolates per-row session counter updates while preserving the enclosing transaction.

Observed run: batch = 17,036,757 ns; 50×append_message = 18,907,933 ns; ratio = 0.901× (batch / append). Test passed its <=2.0× regression bound. This is a small single-run smoke measurement, not a general performance guarantee. Message inserts are still per-row; stock SQLite consolidates session counter updates into one UPDATE at batch end.

## Consumer-scale re-measurement (DF-HRS-112, 2026-10-19)

The 2026-10-17 dogfood doc claimed "2000 messages = 19.48 s at 103 msg/s (one commit per message) vs 0.048 s / 41,270 msg/s in one transaction = 402x" and implied the batch API would close that gap. That claim is RETRACTED and replaced with measured evidence (`batch-2000-wire.log`):

1. The original prose numbers did not even match its own log (`bulk-ab.log` records 27.636 s / 72 msg/s vs 0.047 s = 592.7x).
2. The 402x/592x figure compared one-durable-commit-PER-RPC latency against bare stdlib — a comparison of commit overhead, not of the per-message path the batch API changes.

Measured 2026-10-19 on this host, release-profile adapter at git c4d1e1f, driven over the real stdio JSON-RPC wire by `python3 scripts/hrs003_e2e_proof.py --mode bulk-wire --bulk-n 2000 --bulk-runs 3` (protocol repeated 3x, medians reported; all samples in the log):

- Arm A — 2000 append_message RPCs (one durable commit per call, the original boundary arm): median 24.271 s (82 msg/s)
- Arm B — ONE append_messages_batch RPC over the wire: median 2.068 s (967 msg/s)
- Arm C — stdlib sqlite3 executemany in ONE transaction (original control): median 0.069 s (28,894 msg/s)

New ratio: batch-on-the-wire is **11.7x faster** than per-message wire appends (24.271 / 2.068). It does NOT reach bare-stdlib speed (30x away) — the remaining gap is the store's real per-row work upstream of the control's bare INSERT (uid minting, counters, FTS sync, the §6.1 contract), and that is expected, not a defect.

No claim beyond these arms is made; the in-process 0.9x smoke above remains scoped to its own 50-message transaction comparison.

## Provenance record (DF-HRS-112)

Every measurement and code/doc change for this task was swept into the
foreman's board-close commit `df096fb` ("board: close DF-HRS-109 …", pushed
to origin/main) through a shared-index amend race: the seven files were
STAGED for this task's own commit when the foreman amended HEAD
(`c4d1e1f → 94b389c`, reflog `commit (amend)`), and the amend committed the
shared index with them. Content is verified byte-identical at `df096fb`
(marker greps on `git show HEAD:<file>`); per repo doctrine the pushed
history was NOT rewritten or amended. The commit message intended for the
work is preserved verbatim below as the audit record; the task's own
message text is this file plus this section.

--- intended DF-HRS-112 commit message (verbatim) ---
test(interop): batch+usage wire proof, re-measure bulk claim (DF-HRS-112)

- scripts/hrs003_e2e_proof.py: new batch-wire leg drives append_messages_batch
  (N=200, asserts first_id>0 / appended==N, durable across a second adapter
  process) and record_model_usage ({"ok":true}; deltas land on the session row)
  end-to-end over the stdio JSON-RPC boundary; new bulk-wire mode re-measures
  the consumer-scale claim on the wire (repeated protocol, medians). The
  default proof now includes the leg; standalone modes build the release
  adapter per the DF-HRS-112 contract.
- tests/hrs003_python_interop.rs: assert BATCH_WIRE_OK / MODEL_USAGE_WIRE_OK /
  HRS112_WIRE_CLEAN markers. RED-proven against the pre-DF-HRS-109 adapter
  (dc1877d source): the leg fails with fail-closed exit 6 at method
  "append_messages_batch" — a dispatch-arm regression now fails the battery.
- .github/workflows/rust-ci.yml: run the wire proof on every push; proof
  script paths added to the trigger list.
- docs/evidence/hrs109/batch-2000-wire.log: N=2000 x3 on the wire —
  per-message append_message 24.271s median (82 msg/s) vs ONE
  append_messages_batch RPC 2.068s median (967 msg/s) = 11.7x; stdlib
  one-transaction control 0.069s.
- docs/evidence/hrs109/throughput.md: the dogfood "402x / 19.48s" framing is
  RETRACTED — its prose figure never matched its own log (27.636s / 592.7x in
  bulk-ab.log) and it compared per-RPC commit overhead against bare stdlib,
  not the per-message path the batch API changes. Replaced by the measured
  wire numbers above.
- docs/dogfood/diagnostics.md + specs/python-interop.md: figures corrected to
  measured values; the §4 wire-method table now lists both §6.1 methods it
  already claimed in §3.1.

Gates: cargo fmt --check; cargo clippy --workspace --all-targets
--all-features -- -D warnings; cargo test --workspace --all-features — all
exit 0.
--- end intended commit message ---
