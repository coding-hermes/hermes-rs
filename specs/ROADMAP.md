# Hermes RS delivery roadmap

## Current state

The pinned upstream reference and scoped Python harness run are recorded in `UPSTREAM_BASELINE.md`. The live board tracks HRS-001 through HRS-009 plus the dogfood findings; `NEVER-DONE` is a fixture, not product work. The current resource-profile numbers are preliminary: the capture implementation needs correction before they can justify architecture or performance claims. There is not yet a Rust crate, executable, or Rust test run.

The finish line is not “a Rust program that passes its own tests.” A supported release must run the upstream Hermes harness against the Python reference and Rust target through an explicit adapter; publish case-level results using the same pinned harness/case IDs and explain every classification; retain SQLite support; prove Python interoperability at a named boundary; and ship tested install, upgrade, backup, migration, and rollback paths.

## Delivery sequence

### Gate 1 — Make the Python resource baseline decision-grade (HRS-006)

Repair the measurement harness before using it to choose an architecture. Measure actual batch elapsed time, correct per-request percentiles, process-tree CPU and RSS, end-of-run RSS, SQLite storage growth, and variance. Use deterministic provider replay for local cost and keep a separate live-provider run for end-to-end latency. Include a continuing session, tool activity, and persistence workload at concurrency 1 and at least 4. Store raw runs and full environment/workload provenance. Until this gate passes, the existing P1/P2 results are historical observations only—not evidence that Rust or another database is faster.

### Gate 2 — Establish the upstream-to-Rust test contract (HRS-007)

Pin the upstream release and canonical runner. Inventory stable test IDs and classify tests that are observable through a target adapter, tied to Python internals, provider/live dependent, platform dependent, or excluded with a reason. Keep the total and classification counts visible; never silently reduce the denominator. Prove the adapter fails when the Rust target is deliberately broken, so a Python fallback cannot create a false Rust pass. Do not modify upstream tests merely to make the Rust target pass.

### Gate 3 — Specify and implement stock SQLite persistence (HRS-002, HRS-008)

Specify a Rust persistence boundary from the pinned upstream schema and behavior: transactions, identifiers, FTS5, WAL/checkpointing, migrations, concurrent access, backup, and recovery. Standard SQLite remains a supported backend and the compatibility baseline. Then build the first Rust vertical slice around session persistence: open a copied Python-produced database, perform the supported operations, reopen it, and verify the same observable outcomes through the upstream test adapter. Preserve an explicit migration/rollback path. Do not claim full Hermes parity from this slice.

### Gate 4 — Prove a Python integration (HRS-003)

Inventory Python packages and extension points from the pinned upstream source. Choose the adapter boundary only after naming the actual package and its requirements. Prove one real integration through the Rust-controlled runtime, including data conversion, errors, lifecycle, and installation. State clearly whether support is process/protocol based or in-process; one example does not establish universal CPython ABI compatibility.

### Gate 5 — Evaluate optional storage choices on the same workload (HRS-009)

After the SQLite path works, replay the corrected Hermes-shaped workloads against stock SQLite and a SQLite-compatible Rust candidate such as Turso. Compare behavior, durability, recovery, throughput, latency, CPU, RSS, and storage growth under identical conditions. Keep SQLite supported regardless of the result. If PostgreSQL is offered, provide and test SQLite-to-PostgreSQL and PostgreSQL-to-SQLite migration, including integrity checks and rollback after new writes. PostgreSQL and Turso remain optional until their gates pass; no default switch is implied by a benchmark result.

### Gate 6 — Expand parity by subsystem, then certify releases (HRS-004)

Implement one upstream subsystem at a time. For every release, compare the pinned Python and Rust targets through the same harness, retain raw per-case output and the pinned harness SHA, publish supported behavior and known gaps, and turn relevant upstream changes into traceable work. Gate releases on compatibility evidence, clean-machine installation, security review, data migration/rollback, and documented support versions. Start upstream change tracking early; do not wait for broad parity to begin maintaining the pin.

## Execution rules

- Every task names paths, test evidence, dependencies, and an observable completion condition on the board.
- Rust-native tests supplement the upstream harness; they do not replace it.
- A `SKIP`, unsupported case, or platform/provider limitation stays visible with a reason; it never disappears from the report.
- The first Rust commit must enable the Rust test guard and prove the Rust CI workflow actually runs.
- No full drop-in, performance, or database-speed claim until the exact acceptance gate has passed on the published commit.
- Any shared scheduler change needed for lanes is a separate, owner-approved change; never bypass its API validator.

## Continue / stop reviews

At each gate, publish the evidence and a short continue / re-scope / stop recommendation. Before setting numeric performance targets, establish the corrected Python baseline and have the owner approve the workload and thresholds. Stop or narrow performance work if representative, provider-isolated workloads do not show a material resource problem; do not invent an efficiency target after seeing the results. Stop a release claim on any unexplained supported-case regression, missing migration/rollback proof, or non-reproducible test denominator.
