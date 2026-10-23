# hermes-rs dogfood — run 4 (2026-10-23): a two-year archive, at scale, through the documented Python boundary

**Target:** `hermes-rs` @ `f7fba49` (`main`, tree clean, `origin/main` in sync)
**Lane:** `hermes-rs-dogfood-2026-10-23-19-00-13`
**Verdict:** 🟡 PROMISING-BUT-ROUGH — the persistence slice is SHIPPABLE at realistic
scale; what a new user meets first (README, the reference client, the usage skill)
still misdescribes or hides it.

No product code and no board state was modified by the run itself; every finding
is a row the owning foreman works.

## 1. The promise (null hypothesis)

From `README.md`, `specs/python-interop.md` §3.4/§4 and `skills/hermes-rs-usage`:

> A Python program using **only the CPython standard library** can create and query a
> Hermes-shaped SQLite session archive through a subprocess JSON-RPC boundary, with
> structured typed errors, durable writes, and a file format any SQLite can read.

That is what run 4 drove, on a workload with the shape a real user's archive has,
not on the project's own fixtures.

## 2. What was actually done (real use)

Scenario: *"I have two years of agent transcripts. Put them in one store, then let me
find things in them, and let me hand a copy to someone."* The consumer is a normal
Python script (`docs/evidence/dogfood-20261023/run_user_flow.py.txt`) that imports the
repo's own documented reference client (`scripts/hrs003_client.py`) and nothing else.

| Stage | Result | Captured |
|---|---|---|
| corpus | 300 sessions × 200 messages = **60,000 messages** (10.9 MB of text) | `runA.ndjson` |
| create + open (cold, new store) | **1.589 s**, `created_new_store: true`, schema `31`, `degraded: []` | `runA.ndjson` |
| import (300 batch calls, one per session) | **18.854 s** total = **0.314 ms/message**, **0 errors** | `runA.ndjson`, `runA.time.log` |
| process cost | wall 21.14 s, user CPU **0.60 s**, max RSS 43 MB | `runA.time.log` |
| search, 10 real user queries | all 10 found their planted targets; per-query 0.2–8.7 ms | `runA.ndjson` |
| reopen the same store from a NEW process | **0.151 s** cold; first search 0.86 ms | `tail.ndjson` |
| warm search on the reopened 59 MB store | p50 **0.14 ms**, max 0.39 ms (20 samples) | `tail.ndjson` |
| `get_messages` (200 rows) | 1.72 ms | `tail.ndjson` |
| backup of the live 59 MB store | **1.845 s**, destination byte-identical size | `tail.ndjson` |
| integrity, system SQLite 3.53.4 (a different build than the crate's bundled one) | `ok`; messages 60000, sessions 300, FTS rows 60000 — on **both** files | §4 |
| counters vs reality | `message_count` 200 = actual rows 200 on 3 sampled sessions; `started_at` within ~20 ms of the first message | `tail.ndjson` |

Store size: **59,031,552 bytes for 60,000 messages ≈ 984 bytes/message**, of which the
FTS index is the bulk (base table + `messages_fts` each hold 60,000 rows).

## 3. What a user learns about search (documented surface, driven for real)

The usage skill says only "search is token-based, never substring". Driving it with the
queries a user actually types on a *control* store whose contents are known exactly
(`tail.ndjson`, stage `multiword_semantics`; 4 messages: only `pool`, only `connection`,
both adjacent, both separated by other words):

| Query | Hits | What it means |
|---|---|---|
| `connection pool` | 2 (ids 3, 4) | multi-word = **AND** of tokens |
| `connection AND pool` | 2 | explicit `AND` is accepted |
| `connection OR pool` | 4 | explicit `OR` is accepted (undocumented but useful) |
| `"connection pool"` | 2 — **including the message with words between them** | quotes do **not** mean phrase/proximity |
| `pool connection` | 2 (same rows) | order-insensitive |
| `connect*` | 3 (ids 2, 3, 4) | trailing `*` = prefix, as the skill states |

Two consequences for a real user: `AND`/`OR` work and are worth documenting; a quoted
phrase silently behaves like AND, so a user who quotes to get an exact phrase gets loose
results with no signal. The skill was corrected in this run (§5) to state all of this.

Search hits carry `message_id`, `rank`, `role`, `session_id`, **`snippet`** with the match
highlighted as `>>>term<<<` — i.e. the surface is display-ready:

```
{"message_id":16820,"rank":-9.5499,"role":"assistant","session_id":"sess-2024-0084",
 "snippet":"connection cost >>>4096d<<< query message rowid model buffer planner …"}
```

## 4. Trust checks (did anything break?)

- `pragma integrity_check` → `ok` on the 59 MB archive **and** on its backup, read with the
  host `sqlite3` 3.53.4 — a different SQLite build from the crate's bundled one.
- Row counts agree across base table, session table and FTS index (60000 / 300 / 60000);
  no orphan FTS rows.
- Counters agree with reality on sampled sessions (200/200) — the projection is not
  drifting from the message rows.
- The process exits clean on `shutdown` with no leftover `-wal`/`-shm` (verified by a
  fresh process reopening the same file and answering a search).
- Backup destination is a complete, independently openable copy (same size, integrity ok,
  60000 rows readable).

Nothing was lost or corrupted. This slice survives restart, backup and cross-implementation
reads.

## 5. Findings (rows filed)

1. **`DF-HRS-113-R2`** (recurrence of the closed guard row) — the DF-HRS-113 guard
   (`scripts/check_skill_claims.py`) checks *staleness markers* (`open|still|yet|workaround|…`),
   so a **confident present-tense behaviour claim** against a `complete` row passes CI. It
   did: the usage skill still taught that `create: "yes"` behaves as absent and that
   `read_only: "true"` yields a write-capable handle, while `04822cc` (2026-10-20 11:29,
   fourteen minutes *after* the skill was rewritten at 11:15) made both `InvalidInput`.
   Verified live at HEAD (`flags.ndjson`):

   | arm | reply |
   |---|---|
   | `create:"yes"` | `InvalidInput: params.create must be a JSON boolean; got string` |
   | `create:1` | `InvalidInput: params.create must be a JSON boolean; got number` |
   | `read_only:"true"` | `InvalidInput: params.read_only must be a JSON boolean; got string` |
   | `read_only:true` (control) | opens; write → `InvalidInput: read-only store: attempt to write a readonly database` |
   | `create:true` (control) | creates, `degraded: []` |

   `python3 scripts/check_skill_claims.py` → **rc=0, no violation**. The skill prose was
   corrected in this run; the row is the guard coverage, not the sentence.

2. **`DF-HRS-118`** — `README.md` "Status" materially misdescribes HEAD: *"contains only a
   buildable crate scaffold; it does not implement Hermes Agent functionality"*. HEAD ships
   a 4,931-line persistence engine (`src/persistence/stock_sqlite.rs` 1,748; `api.rs` 521;
   `upstream_ddl.rs` 723; adapter 669), two binaries, a documented Python wire protocol, 12
   integration test files, `specs/`, and `skills/hermes-rs-usage`. A visitor reading the
   README concludes there is nothing to use and leaves — the real entry points are
   undiscoverable from the file every visitor reads first.

3. **`DF-HRS-119`** — the docs name `scripts/hrs003_client.py` as "the reference stdlib
   client", but it has no `get_session` method, which `specs/python-interop.md` §4 lists on
   the wire. A user following the table hit `AttributeError: 'SessionAdapter' object has no
   attribute 'get_session'` on their first real query (captured in `runA.time.log`; the run
   then continued via raw frames). Also absent: `hello`, `ping` (cosmetic). Client methods:
   `open, ensure_session, append_message, append_messages_batch, record_model_usage,
   get_messages, end_session, search, checkpoint, backup, close_store, shutdown`;
   wire table: the same **plus `get_session`, `hello`, `ping`**.

## 6. Install leg (fresh machine) — ran, not a silent pass

`bunker-qa.sh launch` → agent `bf15df01` on `bunker-mvp`, QA HEAD `f7fba49` (ff-only pull
ok), then `collect` → 24 evidence rows, agent destroyed (`bunker list` census: 0 matches).

| cell | verdict | detail |
|---|---|---|
| `fresh-install` | **OK** | `Finished dev profile [unoptimized + debuginfo] target(s) in 18.09s` on a bare agent |
| `ci-pass` | **OK** | `act: 1 jobs green — rc=0` |
| `chaos-resource` | **PASS** | suite survives a 3 GB memory cap (child saw 3145728 KB) |
| `upgrade` | N/A | no release tags in repo — consistent with "no installable artifact" |
| `docker-deploy`, `chaos-shutdown`, `ui-probe` | N/A | no compose file / no web UI |
| `chaos-disconnect` | UNVERIFIED | `disc.log` empty — the cell produced no output; harness class, unchanged from earlier runs |
| `chaos-corruption` | N/A | battery reports "no db/state files in repo" — for a *SQLite persistence* project that detection is wrong: the product creates its DB at runtime, so the corruption cell never runs. Harness blind spot, recorded here, not filed against the product |
| `chaos-errorpath` | INFO | entrypoint not observable |
| `toolchain-bootstrap` | OK | go 1.27.3, rust stable, gcc/g++ wrappers, make; `INFO` bare agent lacks `libssl-dev` (rust cells on that dep grade ENV-BLOCKED) |

So: a fresh machine builds this from a clone with no extra variables, and its CI job is
green there. Installability: **proven**.

## 7. Performance (Step 2b) — measured, nothing worth a row

| operation | cold | warm |
|---|---|---|
| create a new store | 1.589 s | — |
| open a 59 MB / 60k-message store | 0.151 s (new process) | 0.168 s |
| import 60,000 messages | 18.854 s (0.314 ms/msg) | — |
| search (single term, 60k docs) | 0.86 ms (first query after open) | **0.14 ms p50**, 0.39 ms max |
| `get_messages` 200 rows | — | 1.72 ms |
| backup 59 MB | 1.845 s | — |

No `PERF-` row is filed: at 60k messages every user-facing operation is sub-millisecond
except the one-shot import (19 s for two years of history, 0.31 ms/message) and first-ever
store creation (1.59 s, once). A user would not notice any of these; a win nobody can feel
is not a finding. CPU column confirms the import cost is commit I/O, not compute
(user 0.60 s for 18.85 s wall).

## 8. Verdict

🟡 **PROMISING-BUT-ROUGH.** Asked "does it actually work", the answer from real use is yes
for the persistence slice: 60k-message archive, 0.31 ms/message import, sub-millisecond
token search, honest fresh-store report, counters that match reality, byte-identical
integrity-checked backup, clean reopen, cross-SQLite readability, and a fresh-machine
install with green CI. What is rough is everything a new user touches *first*: the README
says the repo is an empty scaffold, the reference client cannot call one of the documented
methods, and the project's own usage skill taught a behaviour the code had already
replaced — with a CI guard that cannot see that class of drift.

Engine: SHIPPABLE at this scale. Delivery surface: PROMISING-BUT-ROUGH.
