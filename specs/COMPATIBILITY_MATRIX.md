# Compatibility inventory

This matrix is the source of truth for scope. Each row must name the upstream tag/commit examined, source location, contract fixture, Rust status, Python interoperability requirement, last checked release, and owning task. `Untriaged` means no parity claim is permitted.

| Surface | Upstream reference | Rust status | Python interop | Contract evidence | Task |
|---|---|---|---|---|---|
| CLI and process lifecycle | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-001 |
| Configuration and environment resolution | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-001 |
| Agent loop / orchestration | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-001 |
| Provider and model routing | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-001 |
| Tools and tool-call protocols | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-001 |
| MCP integrations | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-001 |
| Persistence and database access | Upstream baseline pending; retain SQLite | Untriaged | Untriaged | Compare stock SQLite and optional Turso; PostgreSQL optional, migration round-trips required | HRS-002 |
| Python libraries / extensions | Untriaged | Untriaged | Must be categorized | Adapter proof pending | HRS-003 |
| Gateway / API / streaming contracts | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-001 |
| Memory / session / history semantics | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-001 |
| Install, upgrade, and release behavior | Untriaged | Untriaged | Untriaged | Pending baseline | HRS-004 |
| Observability, limits, and resource controls | Untriaged | Untriaged | Untriaged | Baseline benchmark pending | HRS-002 |

## Update rule

For each upstream release, record the tag/commit and review changed files, APIs, configuration, and tests. Add/adjust rows and tasks before claiming the release is tracked. Do not convert an unknown row to supported without a fixture or an explicit, reviewed exclusion.
