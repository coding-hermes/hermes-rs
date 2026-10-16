# HRS-009 benchmarks — stock SQLite vs Turso 0.8.3

Reproducible workload behind the engine-neutral persistence shim
(`src/persistence/api.rs`). Every number here traces to a checked-in raw
JSON record; regenerate with:

```sh
sh scripts/run_hrs009_bench.sh <repo>          # refuses loadavg > 4
# arms:
cargo run --release --example hrs009_bench -- --backend stock --out benchmarks/results/hrs009-stock.json
cargo run --release --features turso --example hrs009_bench -- --backend turso --out benchmarks/results/hrs009-turso.json
```

Workload contract (FIXED before any run; no tuning pass was made after
seeing numbers — measurement discipline per the HRS-009 brief):

- Dataset: 5,000 sessions × 2–10 transcript messages, 200–500 words per
  message (a few KB per session), deterministic xorshift64* generator
  (seed recorded in the JSONs), written once through the STOCK backend;
  both arms open the SAME file. The file is a real Hermes-shaped store:
  full upstream schema INCLUDING the FTS5 virtual tables the stock
  backend creates — 323,117,056 bytes.
- Workload: 10,000 ops, 70% reads (point reads by session id + message
  page) / 30% writes (ensure + append; usage delta every 25th write;
  PASSIVE checkpoint every 500 writes).
- Recorded per backend: per-op latency p50/p95/p99 (reads and writes
  separately), throughput ops/s, wall clock, CPU consumed by the workload
  (`cpu_seconds_total` from a /proc/self/stat utime+stime delta read
  before/after the run; `cpu_percent_avg` = cpu_seconds / wall_seconds ×
  100 — same convention as the hrs006 baseline records), peak RSS (VmHWM
  from /proc/self/status), final storage bytes (main + WAL sidecars), and
  the per-op error list (the harness stops after >20 errors rather than
  spinning).

Environment at measurement time (also embedded in each JSON):

- Host: master001 — AMD Ryzen 9 7950X 16-Core Processor (32 hardware
  threads), Debian 13 (trixie), kernel 6.12.96+deb13-amd64, NVMe,
  loadavg 0.39 at run time (the runner refuses loadavg > 4). The CPU
  fields were added in the HRS-009 rework and BOTH arms were re-run
  back-to-back on the SAME fixed dataset (fresh-deterministic rebuild,
  identical storage_bytes); the non-CPU metrics below are from that
  re-run and moved only within run-to-run noise.
- Toolchain: rustc/cargo 1.99.1 stable; release profile.
- Crate versions (from Cargo.lock at the measured commit):
  rusqlite 0.40.2 (bundled libsqlite3, FTS5+trigram amalgamation),
  turso 0.8.3, tokio 1.x (rt+time features only).

## Results

Raw records: `benchmarks/results/hrs009-stock.json`,
`benchmarks/results/hrs009-turso.json`. The analysis in
`specs/backend-evaluation.md` §2 cites these files; where this README and
a JSON disagree, the JSON is authoritative.

| Metric | Stock SQLite | Turso 0.8.3 |
|---|---|---|
| Ops completed | 10,000 / 10,000, 0 errors | 49 / 10,000, 22 errors, then circuit-breaker stop |
| Wall clock | 0.98 s | 1.70 s (49 ops) |
| Throughput | 10,205.6 ops/s | 28.8 ops/s (aborted run — not comparable) |
| CPU (`cpu_seconds_total`) | 0.980 s | 1.700 s (for the 49 ops it managed) |
| CPU (`cpu_percent_avg`) | 100.0% | 100.0% |
| Read p50 / p95 / p99 (ms) | 0.041 / 0.056 / 0.062 | 34.2 / 36.3 / 38.6 |
| Write p50 / p95 / p99 (ms) | 0.172 / 0.280 / 0.824 | none committed (every write errored) |
| Peak RSS (VmHWM) | 7,668 KB | 40,344 KB |

**Honest losing number: Turso is the loser on p95 — on the identical
Hermes-shaped workload its read p95 is ~649× stock SQLite's (36.3 ms vs
0.056 ms) and it committed zero writes before its error circuit-breaker
stopped the run — while burning 1.7× stock's CPU time (1.700 s vs
0.980 s) to do it.**

Root cause (measured, minimized in specs/backend-evaluation.md §3.2):
the dataset is a real Hermes store WITH FTS5 objects. Turso 0.8.3
mis-resolves schema on files containing FTS5 virtual tables — every
`ensure_session` upsert fails with `Parse error: ON CONFLICT clause does
not match any PRIMARY KEY or UNIQUE constraint` on an unrelated plain
table, and reads degrade to full-scan costs. The identical upsert
succeeds via sqlite3 on the same file, and via turso on a turso-created
file without FTS5 — the failure is turso's, not the harness's.

## Caveats (read before quoting any number)

1. The turso number describes turso ON A REAL HERMES FILE — the only
   configuration that matters for this project. Turso on its OWN schema
   (no FTS5 objects) runs the workload (probe evidence in
   backend-evaluation §3.2), but such a store cannot host upstream data,
   so those numbers were not pursued as a headline.
2. Single-process-per-engine; concurrency beyond 2 threads in one process
   was out of scope (see parity test p8 for the 2W+2R behavior and
   backend-evaluation §3.4 for the cross-process limitation).
3. These numbers describe THESE binaries on THIS host with THIS seed. No
   drop-in, upgrade, or general speed claim follows from them in either
   direction; stock SQLite remains the default regardless.
4. The turso arm runs the engine through a sync shim (block_on per op) —
   that is the shape any real caller of this crate would use from the
   existing synchronous codebase; a native-async caller is out of scope
   for Hermes RS.
