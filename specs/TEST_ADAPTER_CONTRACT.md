# Test Adapter Contract (HRS-007)

| Field | Value |
|---|---|
| Status | COMPLETE |
| Date | 2026-10-15 |
| Task | HRS-007 |
| Upstream pin | v0.22.0 @ `e06df9c2eae6901f4c88f464e0ae6e753c0428b8` (checkout: `/tmp/hermes-upstream-pin-hrs001`, read-only) |
| Companion docs | `specs/UPSTREAM_BASELINE.md` (pin + harness), `specs/HERMES_RS_COMPATIBILITY.md` (contract), `specs/COMPATIBILITY_MATRIX.md` (matrix) |
| Versioned manifest | `specs/TEST_CLASSIFICATION_MANIFEST.json` (schema `hermes-rs/test-classification-manifest`, version 1) |
| Prototype | `prototypes/test_adapter/` (this repo; upstream untouched) |

## 1. Purpose

Define how the pinned upstream Hermes test suite will be run against a
future Rust implementation ("the target") so that:

- every paired run has an exact, versioned denominator;
- every case is classified before it is run, with the class deciding
  whether a Rust paired result is even meaningful;
- the target cannot "pass" by Python substitution, absence, or crash —
  the adapter is fail-closed by construction.

Nothing here modifies upstream. All adapter-side artifacts live in this
repo and are versioned with it.

## 2. Scope and denominator

The classification covers the **scoped subset** defined by HRS-001
(`specs/UPSTREAM_BASELINE.md` section 5): `tests/providers/` +
`tests/conformance/` — **21 test files** — the same set used for the
HRS-001/005 scoped runs.

- Collection (pinned checkout, its own `.venv`):
  `HERMES_HOME=<isolated> .venv/bin/python -m pytest --collect-only -q
  -p no:cacheprovider tests/providers/ tests/conformance/`
- Collection environment: CPython 3.14.5, pytest 9.1.1,
  pytest-asyncio 1.3.0 (upstream dev pins).
- Collected cases: **91** (`test_file.py::test_id`, parametrized ids
  rendered by pytest exactly as collected).
- Reconciliation: `totals.collected == totals.classified ==
  sum(by_class) == len(cases) == 91`, enforced by the generator
  (`prototypes/test_adapter/generate_manifest.py`), which re-collects
  from the pin on every run and FAILS CLOSED if the embedded
  classification and the live collection ever diverge (this fired once
  during authoring — two parametrized ids were mis-guessed and the
  generator refused to emit the manifest until they matched).

The full 5,724-file suite is out of scope for this pass; extending the
manifest is additive (new scope block + cases, version bump).

## 3. Classification

Vocabulary (mutually exclusive; strongest constraint wins):

| Class | Meaning | Count |
|---|---|---|
| `observable` | pass/fail computable from adapter-visible I/O: request kwargs/URL, registry+filesystem state, local fake servers (127.0.0.1 HTTP), zero-request assertions | 57 |
| `python-internal` | verifies Python process/import/registry-cache semantics (import order, entry-point loader mechanics, module-singleton memoization) with no cross-language observation point | 21 |
| `platform-host` | outcome depends on OS process semantics beyond an adapter boundary: real SIGKILL to child processes, multi-process races under file barriers, on-host journal-mode resolution | 13 |
| `provider-live` | contacts an external provider API without a local fake | 0 |
| `explicitly-excluded` | excluded from paired runs with a recorded reason | 0 |

57 + 21 + 13 + 0 + 0 = 91. Reconciliation is machine-checked in the
manifest (`reconciliation.checks`) and re-asserted by
`prototypes/test_adapter/test_fail_closed.py::test_manifest_reconciles_and_pins`.

Every case carries a one-line `evidence` citation (what in the test
source grounds the class). Examples:

- `observable`: `test_transport_parity.py::TestKimiParity::test_thinking_enabled`
  — `ChatCompletionsTransport.build_kwargs(...)` asserted in-process;
  no network. Adapter target: input profile+messages → exact request kwargs.
- `python-internal`:
  `test_auth_registry_import_order.py::test_tui_import_exposes_auth_registry_to_provider_plugins`
  — spawns a fresh CPython child importing `hermes_cli.auth` and a
  Python plugin file; the verdict is about Python import-order semantics.
- `platform-host`:
  `conformance/persistence/test_cell1_prefix_durability.py` — the
  upstream harness (`_harness.py`) delivers **real SIGKILL** to a
  separate OS process mid-write and polls file barriers.

Notes on boundary calls (deliberate, recorded per case):

- Cases that spin a **loopback fake server** (PKCE fake IdP, models-list
  fakes, redirect-credential rig) are `observable`: the entire
  request/response conversation happens against 127.0.0.1 and is fully
  reproducible by an adapter-side fake.
- Cases that **monkeypatch a mock and assert a Python-internal call**
  (`test_picker_passes_base_url`, registry cache/TTL/memo tests,
  entry-point loader tests) are `python-internal`: the observation
  point is a Python object, which a Rust target does not have.
- `test_committed_vectors_match_regeneration` is `observable` (file
  equality), but its Rust analogue regenerates from Rust oracles — see
  §7.
- Upstream conditional skips (e.g. the persistence cells' `[WAL]`
  params skip when the host resolver downgrade-gates trip) are recorded
  as `platform-host` with the skip condition in the evidence; if a
  paired run hits the skip, the result format records `SKIP-EXCLUDED`
  with the upstream skip reason (never a pass).

## 4. Harness interaction contract (upstream runner ↔ adapter)

Facts verified at the pin; each clause states how a Rust-target adapter
must behave.

### 4.1 Per-file isolation (`scripts/run_tests.sh` + `scripts/run_tests_parallel.py`)

Each test FILE runs in its own freshly-spawned `python -m pytest <file>`
subprocess; no xdist, no shared workers. Determinism env
(`TZ=UTC LANG=C.UTF-8 PYTHONHASHSEED=0`) is exported per run. Direct
`pytest tests/...` invocation is NOT a valid parity baseline: HRS-001
measured 19 cross-file-pollution failures under one-process pytest that
pass per-file (recorded in `specs/UPSTREAM_BASELINE.md` §5).

Adapter consequence: **the Python arm of any paired run MUST be invoked
through `scripts/run_tests.sh`** (per-file subprocesses). A Rust arm has
no pytest, but the paired unit of comparison is still the per-file
case id — the adapter maps Rust-side results back onto those ids, and
the report must state that the Python side ran under the canonical
runner.

### 4.2 conftest env handling (`tests/conftest.py`)

- Sandboxes `HERMES_HOME` **before any test module imports** (collection
  time), redirects `Path.home()`, refuses a production home, and
  registers an isolated session home via `HERMES_TEST_SANDBOX_HOME` +
  atexit cleanup. `HERMES_TEST_ISOLATION` is exported for children.
- An ambient/production `$HERMES_HOME` is a known failure mode
  (HRS-001 §5: 19 `AttributeError: 'NoneType'` profile failures on a
  shared host).

Adapter consequence: every Python-arm invocation (including the
collection step that produces denominators) must set an isolated
`HERMES_HOME`. The Rust arm must receive the same logical inputs: the
adapter hands the target an isolated home root, never the host's.

### 4.3 Nested subprocesses

Two shapes exist in the scoped subset:

1. **Plugin-probe children** (`test_auth_registry_*`): `subprocess.run`
   of `sys.executable -c <probe>` with `cwd=REPO_ROOT`,
   `HERMES_HOME` + `PYTHONPATH` set explicitly, output captured.
2. **Crash-conformance children** (`conformance/persistence/_harness.py`):
   `subprocess.Popen(sys.executable -c <script>)`, PYTHONPATH prepended,
   file-barrier coordination, hard 60 s deadlines, real SIGKILL, stderr
   captured for diagnostics.

Adapter consequence: a Rust target must reproduce the CHILD-SIDE
semantics these children exercise (state-store CLI/library entry, config
resolution from `HERMES_HOME`), not the parent's pytest environment.
For `platform-host` cases the adapter does not run the upstream test
unchanged against Rust; it runs its own equivalent probe (same crash
points, same barrier discipline) against the Rust store and pairs the
RESULTS, with the harness SHA cited on both arms.

### 4.4 Fixtures

Fixture grammar observed in the subset (all adapter-relevant):

- `monkeypatch.setenv("HERMES_HOME", tmp_path)` — env-scoped homes.
- `monkeypatch.setattr(module, "_REGISTRY", ...)` — module-singleton
  resets (this is what makes several cases `python-internal`: the
  observation point is the patched module attribute).
- `tmp_path` trees written as real plugin dirs
  (`plugins/model-providers/<name>/{__init__.py,plugin.yaml}`) — the
  Rust target consumes the same directory contract.
- `autouse` fixtures that evict `importlib.metadata` caches
  (`test_entry_point_discovery.py`) — Python loader hygiene.
- `ProfileTripwire` — snapshot/re-walk of the default-profile tree;
  the verdict is a filesystem diff (adapter-observable).

Adapter consequence: fixture INPUTS are portable (temp dirs, env vars,
plugin trees); fixture MECHANISMS that patch Python modules are not.
The adapter exposes the portable inputs to the target through a plain
invocation contract (config dir, plugin dir, vector input) and never
depends on pytest fixtures running inside the target.

### 4.5 Output capture

Upstream captures child stdout/stderr for diagnostics, and verdicts are
assertions in-process — there is no machine-readable per-case result
artifact upstream. The adapter therefore treats the Python arm's pytest
exit + `-rA`/junit output (when requested) as the Python result source,
and requires the Rust arm to emit the structured result format of §5.
Nothing in the upstream tree is written to.

### 4.6 Excluded-from-default markers

Upstream `addopts = "-m 'not integration and not live'"` excludes
integration/live tests from default runs; the scoped subset contains
none (verified: no `pytestmark`/markers in the 21 files besides
`parametrize`/`platforms("linux")`). The `platforms("linux")` marker on
the persistence cells is why they are `platform-host` (process semantics)
rather than portable.

## 5. Versioned paired-run result format

Schema `hermes-rs/paired-run/1` (emitted by
`prototypes/test_adapter/paired_run.py`, sample committed at
`prototypes/test_adapter/last_paired_run_broken.json` / `..._ok.json`):

```json
{
  "schema": "hermes-rs/paired-run/1",
  "denominator": {
    "total": 12,
    "expected_total": 12,
    "reconciles": true
  },
  "counts": {"passed": 12, "failed": 0, "skip_excluded": 0, "other": 0},
  "harness_sha": "e06df9c2eae6901f4c88f464e0ae6e753c0428b8",
  "harness": {
    "upstream_repo": "NousResearch/hermes-agent",
    "upstream_tag": "v0.22.0",
    "runner": "scripts/run_tests.sh (per-file isolation) per specs/UPSTREAM_BASELINE.md"
  },
  "adapter": {
    "adapter_sha": "<sha256 of adapter.py>",
    "target_module_sha": "<sha256 of the target module>",
    "corpus_sha": "<sha256 of corpus.py>",
    "target_language": "python-stub (stands in for hermes-rs)"
  },
  "host": {"repo_head": "<hermes-rs HEAD>", "python": "3.11.15", "platform": "..."},
  "results": [
    {
      "vector": "<vector id>",
      "status": "PASS | FAIL | SKIP-EXCLUDED",
      "target": "<module name> | unavailable",
      "reason": "<required for FAIL-by-unavailability/crash and SKIP>",
      "verdict": {"checks": {"<field>": {"expected": ..., "actual": ..., "match": ...}}, "match": true}
    }
  ]
}
```

Rules:

- **Exact denominator**: `denominator.total` must equal the declared
  corpus size; `reconciles=false` invalidates the run.
- **Harness SHA**: every run cites the upstream pin
  (`e06df9c2...`), per `specs/UPSTREAM_BASELINE.md` §2.
- **Raw outcomes on both arms**: `results[].verdict.checks` carries
  expected vs actual per field (Python arm's expectation is the vector
  contract; Rust arm's actual is the target's emitted request). Skip
  and exclusion carry reasons, never silent absence.
- **Fail-closed statuses**: unavailable/crashing target → `FAIL` with
  `target: "unavailable"` or the exception text; excluded case →
  `SKIP-EXCLUDED` with reason. No third "unknown" state exists;
  anything unexpected is a FAIL.
- **Versioning**: format bumps change `schema`; the adapter SHA +
  corpus SHA pin the exact binaries that produced a run.

## 6. Fail-closed adapter prototype (proven)

Files (all in this repo, upstream untouched):

| File | Role |
|---|---|
| `prototypes/test_adapter/corpus.py` | 12 contract vectors mirroring 12 `observable` upstream cases (transport parity, e2e wiring, meta-ai profile); constants verified at the pin |
| `prototypes/test_adapter/rust_target.py` | stub "Rust target" standing in for hermes-rs request construction; **ships deliberately broken** (`HRS007-STUB-TARGET-BROKEN`: extra_body fields wrapped in a `__extra_body__` sentinel + a deepseek `max_tokens` leak) |
| `prototypes/test_adapter/adapter.py` | the ONLY sanctioned boundary; sealed against Python fallbacks; fail-closed statuses |
| `prototypes/test_adapter/paired_run.py` | paired-run CLI emitting schema `hermes-rs/paired-run/1` |
| `prototypes/test_adapter/test_fail_closed.py` | 11 tests proving the fail-closed behavior |
| `prototypes/test_adapter/generate_manifest.py` | manifest generator (fail-closed against collection drift) |

Proof, run 2026-10-15 (repo HEAD at authoring: `32fb071`, branch `hrs007`):

```
$ python3 prototypes/test_adapter/paired_run.py          # default = broken target
paired-run: total=12 passed=7 failed=5 skip_excluded=0 -> NOT OK
harness_sha=e06df9c2eae6901f4c88f464e0ae6e753c0428b8 repo_head=32fb0719dc5b
exit code 1
  failing vectors (check-level):
    or-provider-prefs            -> extra_body_provider
    qwen-vl-hi-res               -> extra_body_vl_high_resolution_images
    ollama-numctx                -> extra_body_options_num_ctx
    custom-think-false           -> extra_body_think
    ds-no-forced-max-tokens      -> max_tokens

$ python3 prototypes/test_adapter/paired_run.py --broken false   # correct target
paired-run: total=12 passed=12 failed=0 skip_excluded=0 -> OK
exit code 0

$ python3 -m pytest prototypes/test_adapter/test_fail_closed.py -v
11 passed in 0.32s
```

The three fail-closed guarantees, each with a dedicated test:

1. **Broken Rust target → paired case FAILS**
   (`test_broken_target_fails_every_affected_vector`,
   `test_paired_run_cli_default_broken_exit_1`): every affected vector
   fails with a concrete expected/actual check diff.
2. **No Python fallback can produce a Rust pass**
   (`test_no_python_fallback_can_produce_a_rust_pass`): the adapter
   boundary is SEALED — if any Python implementation module
   (`providers`, `agent.transports.chat_completions`, `run_agent`,
   `hermes_cli.models`, `plugins.model_providers`) is present in
   `sys.modules`, `run_paired` raises `AdapterSealedError` instead of
   running. A Python "helper" cannot silently serve results through the
   boundary that only the Rust target is allowed to fill.
3. **Absence/crash is fail-closed**
   (`test_missing_target_is_fail_closed`,
   `test_crashing_target_is_fail_closed`): a missing target module
   yields `FAIL`/`target=unavailable`; a raising target yields `FAIL`
   with the exception recorded. There is no code path from "target did
   not answer" to "case passed".

## 7. Conformance vectors note (Rust-side oracles)

Upstream's `tests/conformance/` machinery separates the Python oracle
(`format_message` renderers) from the committed vector files. The
`observable` vector-shape cases (`test_vector_files_shape`,
`test_committed_vectors_match_regeneration`) therefore translate to the
Rust side as: regenerate vectors from **Rust** oracles and diff against
the committed corpus, keeping the upstream `oracle.commit`/
`generator_version` provenance block. The generator-health case
(`test_oracles_importable_and_self_free`) is `python-internal` (it
executes the Python renderers unbound) and is not an adapter target.

## 8. What the adapter does not cover (and must never fake)

- `python-internal` (21 cases): import order, entry-point mechanics,
  registry memoization. A Rust implementation has its own loader
  semantics; pairing these would be fiction. They remain upstream-only
  regression coverage of the reference.
- `platform-host` (13 cases): the crash/race contracts transfer
  (acknowledged appends survive SIGKILL; consume-once under process
  races; exactly-once outbox across crash windows), but the executor is
  host OS + CPython + SQLite journal resolution. The adapter plans
  Rust-side equivalents run by the same discipline (§4.3), paired at
  the RESULT level with the harness SHA cited — never by running the
  upstream Python test "against" Rust.

## 9. Upstream integrity

`git diff` against the pinned checkout is empty (verified at commit
time; the prototype reads upstream only via pytest collection and file
reads). Any future target-specific wrapper must live in this repo, as
these files do.
