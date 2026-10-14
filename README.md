# Hermes RS

A resource-efficient Rust reimplementation of Hermes Agent, developed as a separate project and kept behaviorally compatible with the upstream Python Hermes Agent. The goal is a practical drop-in replacement for supported deployments, with Python interoperability where existing Python libraries or integrations require it.

This is an independent reimplementation, not an official Nous Research release. Upstream reference: [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent).

## Principles

- Reimplement one observable subsystem at a time; do not claim parity until contract tests pass.
- Use the pinned upstream Hermes test harness as the common scoring baseline for Python and Rust, alongside Rust-native tests; parity scores must use the same cases and denominator.
- Track upstream releases and changes, mapping each relevant change to an implementation or an explicit compatibility gap.
- Preserve existing configuration, CLI, protocol, storage, and extension behavior where feasible; document intentional differences.
- Keep standard SQLite supported. Route persistence through a backend shim; evaluate SQLite-compatible Rust engines such as [Turso](https://github.com/tursodatabase/turso) by contract tests and Hermes-shaped benchmarks, not assumptions.
- Treat PostgreSQL as optional. If supported, provide round-trip SQLite↔PostgreSQL import/export with tested schema/data conversion and recovery.
- Measure CPU, memory, startup, throughput, and tail latency at realistic scale before optimizing.
- Keep the core Rust-first while supporting existing Python libraries through narrow, tested adapters rather than requiring a wholesale rewrite.

## Status

Project kickoff: architecture/specification and baseline inventory are pending. No drop-in compatibility or performance claims are made yet.

## Development

The repository will document verified Rust toolchain and test commands as the first implementation lands. See `specs/` for the acceptance contract and phased plan.

## License

MIT. See `LICENSE`.
