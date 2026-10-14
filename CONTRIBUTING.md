# Contributing

Contributions should extend the compatibility matrix and use evidence from a pinned upstream Hermes revision. For behavior changes, add or update cross-implementation fixtures before implementation where practical. Never label a feature drop-in compatible without passing its relevant contract tests.

Database proposals must include reproducible workload data, correctness/durability findings, deployment costs, and a migration/rollback plan. Python integration proposals must identify the existing package/API and test the adapter boundary.

Run the repository's documented formatter, lint, unit, integration, and compatibility checks before opening a pull request. These commands will be pinned with the first Rust implementation.
