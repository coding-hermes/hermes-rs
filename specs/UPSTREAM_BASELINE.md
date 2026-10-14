# Upstream baseline (HRS-001)

Source-grounded baseline of the upstream Hermes Agent release that all
hermes-rs compatibility work is measured against. Companion to
`HERMES_RS_COMPATIBILITY.md` (the contract) and `COMPATIBILITY_MATRIX.md`
(the matrix this document underpins). Owned by task HRS-001.

## 1. Pinned upstream release

| Field | Value | How obtained |
|---|---|---|
| Repository | https://github.com/NousResearch/hermes-agent | `git ls-remote` + GitHub API `releases/latest` |
| Latest stable release tag | `v0.22.0` ("Hermes Agent v0.22.0 — The Threshold Release") | GitHub releases API, `prerelease=false`, `draft=false` |
| Annotated tag object SHA | `84094bd903791c35867605e28e25075baa293adc` | `git ls-remote refs/tags/v0.22.0` |
| **Commit SHA (the pin)** | **`e06df9c2eae6901f4c88f464e0ae6e753c0428b8`** | `git ls-remote refs/tags/v0.22.0^{}`; confirmed by clone `git rev-parse HEAD` |
| Commit date | 2026-10-13 15:07:03 -0700 | `git log -1` on the pin |
| Release published | 2026-10-14T03:21:46Z | GitHub releases API |
| Local pinned checkout | `/tmp/hermes-upstream-pin-hrs001` (shallow clone, `--branch v0.22.0`) | read-only reference; NOT part of this repo |

Every matrix row for HRS-001 cites this tag/commit. Any future re-pin must
record a new tag + SHA + date here and re-verify each cited path exists.

### Release notes summary (upstream text, abbreviated)

- ~24,200 commits / ~6,080 merged PRs / ~7,180 issues closed since v0.21.0.
- Source shrank from ~1.06M to ~698K lines (−34%); `gateway/run.py` from
  34,847 to 5,512 lines.
- Guided first-run onboarding on a free trial model (`nous/welcome`).
- Hosted app **connectors** (Gmail, Linear, Notion) via the Nous tool
  gateway; the Connectors page replaces the old MCP tab.
- One-click local models via llama.cpp (CPU/CUDA/Metal/Vulkan/ROCm).
- **Stable SemVer release channels**; source installs follow releases by
  default (`hermes update --set-channel main` for the old behavior).
- Signed desktop packages (MSIX, notarized macOS) and a unified package
  manager (`hermes pm`).
- Plugin catalog (~600 reviewed plugins); memory providers (Honcho,
  Supermemory, Hindsight, Mem0, OpenViking, Holographic) moved to catalog
  plugins. ByteRover and RetainDB remain bundled.
- Claude 5.5 / GPT-6 / GPT-6.1 / Gemini 4 model wave; prior reasoning is
  replayed by default; bounded retries for transient 5xx.
- **One gateway + one desktop backend per machine** serving all profiles
  (multiplexed), replacing one-process-per-profile.
- Upgrade notes relevant to compatibility work: Python 3.14 is the managed
  runtime; per-profile gateways fold into the multiplexed default;
  `session_reset` idle/daily modes retired; `sessions.auto_prune` defaults
  on (90-day inactivity); plugin compat import layer, `hermes sync`, and
  the bundled tirith scanner removed; several tools renamed (`todo` →
  `todo_list`, `cronjob` → `cronjob_manage`, `process` →
  `process_manage`, old names still work).

## 2. Harness inventory (upstream test framework)

- **Framework:** pytest, pinned `pytest==9.1.1` + `pytest-asyncio==1.3.0`
  in `[dependency-groups] dev` of `pyproject.toml` (line ~529) at the pin.
- **Config:** `[tool.pytest.ini_options]` in `pyproject.toml` (line 691):
  `testpaths = ["tests"]`, custom markers (`integration`, `live`,
  `requires_wal`, `no_isolate`, `ssh`, `platforms(...)`, ...), and
  `addopts = "-m 'not integration and not live'"` — integration/live tests
  are excluded from a default run.
- **Canonical runner:** `scripts/run_tests.sh` ("Run this instead of
  calling `pytest` directly to guarantee your local run matches CI
  behavior"). Per-file isolation (each test file runs in its own
  `python -m pytest <file>` subprocess via `scripts/run_tests_parallel.py`),
  `TZ=UTC LANG=C.UTF-8 PYTHONHASHSEED=0` determinism, `-j N` parallelism
  cap. CI invokes exactly this script (`.github/workflows/tests.yml`
  lines 107/196/381).
- **CI lanes:** `.github/workflows/ci.yaml` — "Python tests" (Linux, via
  `tests.yml`), "OS-specific tests" (macOS/Windows via `tests-os.yml`),
  separate `live-providers.yml` for the `live` marker, plus E2E lanes
  (`tests/e2e/`, desktop E2E) that run only on release/manual dispatch.
- **Suite layout** (5,724 `test_*.py` files under `tests/` at the pin;
  largest areas): `tests/hermes_cli/` 1,540 · `tests/gateway/` 1,017 ·
  `tests/agent/` 996 · `tests/tools/` 778 · `tests/tui_gateway/` 294 ·
  `tests/cron/` 154 · `tests/hermes_state/` 153 · `tests/e2e/` 144 ·
  `tests/plugins/` 123 · `tests/pm/` 122 · `tests/providers/` 15 ·
  `tests/conformance/` 6, plus `tests/conftest.py` (deterministic runtime
  + `~/.hermes` isolation fixtures).
- **Commands** (against the pinned Python reference):
  - Full local-parity run: `scripts/run_tests.sh` (use `-j 4` on small hosts)
  - Direct pytest equivalent of the default CI set:
    `python -m pytest tests/ -x -q` (inherits `addopts`, so integration+live
    stay excluded)
  - Single area: `scripts/run_tests.sh tests/agent/`
- **Install requirements** (from the pin): Python `>=3.11,<3.15`
  (`pyproject.toml` `requires-python`; `.python-version` pins 3.14 for the
  managed runtime — 3.13 is the type-check target under `[tool.ty]`),
  `uv sync` with the checked-in `uv.lock` (all runtime deps are
  exact-pinned), then the dev group above. Type checking uses `ty`,
  linting uses `ruff==0.15.10` (`ruff.toml`).
- **Harness revision (SHA):** the pin commit itself —
  `e06df9c2eae6901f4c88f464e0ae6e753c0428b8`. Harness and source are the
  same revision by construction; paired-run reports (AC-2/AC-9) must cite
  this SHA.

## 3. Upstream source map (surfaces examined)

Paths verified to exist at the pin (checked out at
`/tmp/hermes-upstream-pin-hrs001`):

- CLI / process lifecycle: `cli.py` (1,782 lines), `hermes_cli/main.py`
  (entry point, `[project.scripts] hermes = "hermes_cli.main:main"`),
  `hermes_cli/_parser.py`, `hermes_cli/_launchers.py`,
  `hermes_cli/_update_takeover.py`, `batch_runner.py`,
  `hermes_startup_watchdog.py`.
- Configuration / environment: `hermes_cli/config.py`,
  `hermes_cli/config_defaults.py`, `hermes_cli/config_effective.py`,
  `hermes_cli/config_env_routing.py`, `hermes_cli/config_migrations.py`,
  `hermes_cli/config_providers.py`, `cli-config.yaml.example` (124 KB
  annotated reference config), `.env.example`, `hermes_constants.py`
  (`get_hermes_home`).
- Agent loop: `run_agent.py` (`class AIAgent`, 1,555 lines, docstring
  shows the embedding API), `agent/conversation_loop.py`
  (`run_conversation`, line 1709), `agent/turn_facade.py`,
  `agent/agent_init.py`, `agent/context_engine.py`,
  `agent/context_compressor.py`.
- Provider / model routing: `agent/provider_registry.py` (shared
  name->provider map engine), `agent/provider_base.py`,
  `agent/route_binding.py`, `agent/fallback_cooldown.py`,
  `agent/credential_pool_model_cooldowns.py`, `agent/model_metadata.py`,
  `agent/models_dev.py`, `providers/base.py`,
  `hermes_cli/model_tools`-adjacent `model_tools.py`.
- Tools / tool-call protocols: `toolsets.py` (named tool groups; shared
  `_HERMES_CORE_TOOLS` list), `agent/tool_executor.py`,
  `agent/tool_guardrails.py`, `tools/` (per-tool modules: `terminal`,
  file tools, `browser_*`, `approval_*`, ...), `acp_adapter/` (ACP
  protocol adapter, `acp` extra pins `agent-client-protocol==0.9.0`).
- MCP: `mcp_serve.py` (Hermes as an MCP *server*, `hermes mcp serve`),
  `tools/mcp_tool.py` + `tools/mcp_tool_discovery.py` +
  `tools/mcp_oauth*.py` (MCP *client* side), `mcp==2.0.0` pin in
  `pyproject.toml` (runtime extra `mcp` + dev group),
  `optional-mcps/` catalog.
- Gateway / API / streaming: `gateway/run.py` (6,082 lines at this pin —
  the release notes' −34% rewrite), `gateway/config.py`,
  `gateway/delivery.py`, `tui_gateway/` (desktop backend),
  `agent/stream_delivery.py`, `agent/chat_completion_stream_monitor.py`,
  `hermes_cli/gateway.py` + `hermes_cli/gateway_multiplex_*.py`
  (one-gateway-per-machine multiplexing).
- Memory / session / history: `hermes_state.py` (SQLite state store:
  session metadata, message history, model config, FTS5 search; WAL mode;
  `parent_session_id` compression chains; source-tagged sessions) plus the
  `hermes_state_*.py` module family, `agent/memory_manager.py`,
  `agent/memory_provider.py`, `agent/compaction_*.py`.
- Tests: section 2 above.

## 4. Workload resource profiles

Repeatable profile definitions for AC-8 comparisons. Metadata values
marked "pending measurement" are filled by the capture pass (see
`scripts/capture_baseline.py`, `--profile {single_session,multi_session}`);
no measurement claim is made by this document. Hardware/OS/runtime are
recorded from the baseline machine (`uname -a`, `nproc`,
`python3 --version`, `/etc/os-release`, `/proc/cpuinfo`).

### Profile P1 — `single_session`

- Description: single-session interactive agent loop; one conversation,
  one scripted prompt per turn, tool calls enabled, streaming on.
- Concurrency: 1; sessions: 1.
- Dataset: measured — 3 sequential turns, fixed scripted prompt
  (`"Reply with exactly: OK"`), no tools invoked.
- Duration: measured — 15.337 s total for 3 turns.
- CPU % / RSS / throughput / p50/p95/p99: measured 2026-10-14 via
  `scripts/measure_baseline.py` (HRS-005). Results:
  cpu_percent_avg 52.0, cpu_seconds_total 7.979, rss_mb_peak 52.6,
  latency p50/p95/p99 5093/5094/5162 ms,
  throughput 11.74 turns/min, turns 3/3 OK.
  Authoritative per-run JSON: `baseline/hrs005-p1.json`
  (schema `hermes-rs/baseline/1`, environment block included).
- Measurement context: agent loop run end-to-end via the pinned checkout's
  `hermes -z` CLI against a real model (`ocg/glm-5.3-flash`) served by the
  local 9router gateway (`localhost:20128`); provider/network latency is
  INCLUDED in the numbers (these are end-to-end latency, not compute-only).
  CPU% is RUSAGE_CHILDREN utime+stime over summed wall time. RSS sampled
  at 0.25 s intervals from /proc across the process group. NOT a
  compatibility or speedup claim.
- Storage growth: not measured in this pass — reason:
  `measurement_pending` (single script prompt adds negligible session
  state; a tool-heavy pass will move it).

### Profile P2 — `multi_session`

- Description: multi-session batch with tool calls; N concurrent sessions,
  each running a fixed scripted prompt set with tool calls, batch-style.
- Concurrency: measured — 4 concurrent subprocesses; sessions: 4 (the
  "target 8" from the scaffold was not reached in this pass; per-field
  reason: `session_count_constrained_by_host_model_slots`).
- Dataset: measured — 6 turns total (4 concurrent, then 2), fixed scripted
  prompt, no tools invoked.
- Duration: measured — 36.573 s total for 6 turns.
- CPU % / RSS / throughput / p50/p95/p99: measured 2026-10-14 via
  `scripts/measure_baseline.py` (HRS-005). Results:
  cpu_percent_avg 52.7, cpu_seconds_total 19.265, rss_mb_peak 52.6,
  latency p50/p95/p99 5878/6645/7266 ms,
  throughput 9.84 turns/min, turns 6/6 OK.
  Authoritative per-run JSON: `baseline/hrs005-p2.json`.
- Measurement context: identical machine and provider as P1; same
  end-to-end caveat (provider latency included; CPU% undercounts
  concurrent overlap because it divides cumulative child CPU by summed
  wall time). NOT a compatibility or speedup claim.
- Storage growth: not measured in this pass — reason: `measurement_pending`.

### Upstream check execution record (HRS-005, 2026-10-14)

Recorded against the pinned checkout at `/tmp/hermes-upstream-pin-hrs001`
(v0.22.0 @ `e06df9c2`), env `uv sync --frozen --group dev`, CPython 3.11.15.

| Command | Exit | Result |
|---|---|---|
| `HERMES_HOME=<isolated> HERMES_PYTHON=<pin>/.venv/bin/python scripts/run_tests.sh tests/providers/ tests/conformance/ -q` | 0 | `=== Summary: 21 files, 89 tests passed, 0 failed, 2 skipped (100% complete) in 36.0s (16 workers) ===` |

Full log: `baseline/hrs005-scoped-suite.log` (canonical runner output:
begins with the `HERMES_PYTHON` activation banner, ends with `EXIT=0` and
the quoted Summary line).

Scope rationale: the 5,724-file suite is not runnable in a tick; the
smallest self-contained areas (providers, conformance — 21 test files,
~81 tests) were chosen as the scoped subset. Two environment findings
recorded because they change how any future full run must be invoked:

1. **`$HERMES_HOME` ambient leakage:** `providers.get_provider_profile()`
   reads plugin dirs from the ambient `$HERMES_HOME`. A shared host home
   makes 19 profile/parity tests fail with `AttributeError: 'NoneType'`
   (profile lookup returns None). An isolated empty `HERMES_HOME` fixes
   them. Any upstream check run MUST set an isolated `$HERMES_HOME`.
2. **Cross-file pollution under plain pytest:** running
   `pytest tests/providers/ tests/conformance/` directly (one process)
   still fails 19 tests that pass when each file runs alone — the upstream
   canonical runner (`scripts/run_tests.sh`) exists precisely to run each
   test FILE in its own subprocess. Direct pytest invocation is NOT a
   valid parity check; only the canonical runner's results are recorded
   here.
- Hardware: AMD Ryzen 7 7840HS w/ Radeon 780M Graphics, 16 logical CPUs,
  59 GiB RAM.
- OS: Ubuntu 26.04.1 LTS, kernel 7.0.0-31-generic, x86_64.
- Runtime: CPython 3.11.15 (note: upstream supports 3.11–3.14 and targets
  3.14 for managed installs; the reference-environment choice for paired
  runs must be recorded per run).

## 5. Provenance

- Pin made 2026-10-14 (UTC) by this task (HRS-001) from this machine:
  `git ls-remote --tags` listed all tags; GitHub API
  `releases/latest` returned `v0.22.0` (id 412226006, author teknium1,
  published 2026-10-14T03:21:46Z); `git ls-remote` resolved the annotated
  tag object `84094bd9...` and peeled commit `e06df9c2...`; a shallow
  clone at that tag confirmed HEAD == the peeled commit.
- "Latest stable" = newest non-draft, non-prerelease GitHub release at
  pin time. The repo also carries calendar-versioned tags (e.g.
  `v2026.9.24`) and many `abandoned-rc.*` tags; the SemVer release
  channel is what upstream installs follow by default as of v0.22.0.
- All source-path claims were verified by `ls`/`grep`/`sed` against the
  pinned checkout; nothing was copied from upstream beyond short quoted
  facts (paths, names, pinned version strings, commands). Upstream source
  was not modified.
- No Rust implementation, compatibility, or performance claim is made
  anywhere in this document; all Rust rows remain pending.
