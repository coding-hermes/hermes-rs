# Hermes RS reimplementation and compatibility contract

## 1. Problem statement

Hermes Agent's current Python implementation can impose substantial resource costs at large deployment scale. This project will develop a Rust-first alternative that preserves supported user-visible behavior and interoperates with existing Python libraries and integrations. Work is incremental: inventory upstream behavior, specify contracts, implement one subsystem at a time, and continuously compare against upstream releases. The database layer must preserve SQLite support. Put persistence behind a backend shim, keep standard SQLite as the compatibility baseline, and provide tested import/export tools for SQLite↔PostgreSQL if PostgreSQL is supported. Evaluate faster SQLite-compatible Rust engines (including Turso, formerly Limbo) as an optional backend only after workload, SQL/file-format compatibility, durability, and recovery testing; do not assume they are drop-in replacements or faster for Hermes workloads. PostgreSQL is an optional deployment choice, not a mandatory migration.

## 2. Acceptance criteria

**Project-level completion criteria; subsystem-level criteria must be added before implementation begins.**

- AC-1: Given a named upstream Hermes release, when the compatibility inventory is run, then every in-scope user-facing feature/config/protocol has an owner, source reference, status, and parity test or documented exclusion — proof: committed compatibility matrix with no unclassified in-scope rows. (M)
- AC-2: Given an in-scope behavior and the pinned upstream Hermes test harness, when the Python reference and Rust implementation are run against the same harness version, cases, fixtures, and comparable environment, then results are reported side by side and the Rust score is no lower on the supported compatibility set — proof: automated paired-run report with exact upstream tag, harness revision, case denominator, pass/fail/skip reasons, and reproducible command. (T)
- AC-3: Given an existing Python library integration, when invoked through the documented interoperability boundary, then it can be used without requiring callers to migrate to a Rust-only API — proof: executable integration test using a real supported Python package and documented adapter. (T)
- AC-4: Given standard SQLite, a SQLite-compatible Rust engine (including Turso if its tested feature set is viable), and PostgreSQL as an optional backend, when representative workloads are replayed behind the same persistence boundary, then the report compares SQL/behavior compatibility, throughput, p95/p99 latency, CPU, RSS, storage growth, recovery, and migration/rollback costs; standard SQLite remains supported, and SQLite↔PostgreSQL import/export tools preserve supported data — proof: reproducible benchmark harness, migration round-trip tests, and checked-in results with workload/environment metadata. (T/M)
- AC-5: Given an upstream Hermes release or relevant upstream change, when the release-tracking workflow runs, then it identifies changed source/API/config/test surfaces and creates traceable parity work or records why no implementation change is required — proof: replayable release-diff fixture and generated report. (T)
- AC-6: Given a tagged Hermes RS release, when a supported installation is upgraded, then the documented configuration/data compatibility checks pass and an operator can roll back without silent data loss — proof: upgrade/rollback integration test on a copied fixture installation. (T)
- AC-7: Given a claimed subsystem parity milestone, when the release gate runs, then only criteria with passing automated or artifact-backed evidence are marked complete; unsupported behaviors remain listed as gaps — proof: CI parity summary tied to upstream version and commit. (T)
- AC-8: Given sustained scale workload profiles, when Python and Rust implementations are compared under identical conditions, then resource and latency deltas are published with reproducible workload details and no unsupported blanket efficiency claim — proof: benchmark report plus CI-smokeable regression thresholds. (T/M)
- AC-9: Given every upstream test-harness case that can exercise an observable Hermes contract, when the same harness is run against the pinned Python reference and the Rust target adapter, then the comparative score uses the same test IDs and denominator; Rust must match or exceed the Python baseline for supported behavior, and every excluded or harness-incompatible test is classified with a reason — proof: CI artifact containing both raw result files, mapping/adaptation version, exact harness SHA, per-case comparison, and aggregate scores. (T)

## 3. Edge cases

- Upstream behavior may be undocumented, version-dependent, or inconsistent; capture actual behavior from a pinned release and distinguish it from intended behavior.
- Python packages may require CPython ABI, native extensions, callbacks, async runtimes, or process-global state; adapter support must be explicit and tested, not presumed.
- SQLite may be sufficient for some workloads and bottlenecked for others; benchmark read/write mixes, contention, WAL/checkpoint behavior, durability, and multi-process access.
- A non-SQLite backend may improve concurrency but break SQLite file compatibility, SQL semantics, transaction guarantees, backup/restore, or deployment simplicity; do not switch without migration and rollback evidence.
- Upstream releases can change during implementation; use pinned source revisions and repeatable diff reports.
- Existing user data may contain legacy schemas, partial migrations, or corrupted records; preserve originals and make recovery behavior explicit.
- Resource tests are sensitive to hardware, warmup, concurrency, and dataset size; publish environment metadata and avoid comparing unlike runs.
- Partial subsystem parity must not be marketed as a drop-in replacement; capability negotiation and gap reporting must be visible.

## 4. Non-goals

- Modifying or replacing the upstream Python Hermes repository.
- Claiming affiliation, endorsement, or official status from Nous Research.
- Rewriting every subsystem simultaneously or changing user-facing behavior solely to simplify implementation.
- Requiring all existing Python libraries to be ported to Rust.
- Selecting a replacement database before collecting workload and compatibility evidence.
- Claiming universal drop-in compatibility or performance improvement before measurable acceptance gates pass.
- Breaking existing data/configuration without a documented migration, backup, and rollback path.

## 5. Dependencies and sources

- Upstream implementation and release source: `https://github.com/NousResearch/hermes-agent`.
- Rust toolchain and ecosystem: exact minimum supported version and dependency policy to be pinned in the implementation RFC.
- Python runtime and package ecosystem for interoperability: compatibility versions to be selected from the upstream support matrix and real integration tests.
- Database candidates: SQLite is the baseline; alternatives are selected after workload inventory and benchmark feasibility review.
- Release tracking: upstream tags/releases plus source/API/config/fixture diff tooling.

## 6. Test plan

- Inventory and contract tests pinned to explicit upstream release tags/commits.
- Run upstream Hermes's own test framework against the pinned Python reference and the Rust implementation through a compatible target adapter; report per-case and aggregate pass scores with the same harness version and denominator.
- Keep Rust-native unit, integration, and end-to-end tests as additional coverage; they supplement rather than replace the upstream harness.
- Unit tests for Rust subsystem behavior, parsing, errors, configuration, persistence invariants.
- Cross-implementation contract tests that execute the Python reference and Rust implementation on equivalent fixtures.
- Python interop integration tests for import/call/data/error behavior across the adapter boundary.
- Database correctness, concurrency, crash-recovery, migration, backup/restore, and rollback tests for each candidate.
- Reproducible workload benchmarks: baseline and scale profiles, warm and cold starts, single/multi-process contention, CPU/RSS, throughput, p50/p95/p99, storage and recovery.
- Release-upgrade tests and upstream-change detection fixtures.
- CI gates for formatting, linting, tests, dependency/security checks, compatibility coverage, and release evidence.

## Implementation sequencing

1. Baseline the upstream repository, supported runtime/config/API surfaces, released versions, and current resource hotspots.
2. Freeze a compatibility model and generate an initial subsystem inventory/coverage matrix.
3. Build a representative benchmark suite before architectural optimization; include SQLite baseline and evaluate a database shim boundary.
4. Specify and implement one vertical slice at a time, with upstream parity fixtures and explicit gaps.
5. Add Python interoperability only at the concrete boundaries needed by existing libraries; keep it replaceable and isolated.
6. Add upstream release/change tracking and compatibility alerts; update the matrix for every release.
7. Release only with migration/rollback and support-matrix evidence; make the Python/Rust parity status explicit.
