# hermes-rs integration report — 2026-10-17 (dogfood run 2)

**Target:** hermes-rs @ `887a257` (main), adapter `hermes-rs/session-adapter/1`.
**Angle:** the first real *consumer* of this repo. Run 1 (2026-10-14) exercised the
two scaffold scripts at spec stage; this run builds a Python program **outside
the repository** and drives the published boundary the way a user with existing
Hermes state would. Findings: `DF-HRS-104..111`.

---

## 1. What a user can actually do today

The repo ships three surfaces:

| Surface | Entry point | State |
|---|---|---|
| CLI scaffold | `cargo run` → `hermes-rs` | prints `Hermes RS scaffold: no agent functionality implemented`, rc=0 |
| Session-store library | `hermes_rs::persistence` (`StockSqliteBackend::open`, `SessionStore::write/read`) | HRS-010 slice: the §6.1 operation set |
| **Python boundary** | `hermes-rs-session-adapter` (stdio JSON-RPC) + `scripts/hrs003_client.py` | HRS-003: one proven interop boundary |

The only surface that gives a *user* (as opposed to a Rust contributor) value is
the third one: a Python program that must persist Hermes-shaped sessions into a
`state.db`-compatible file. That is what this report drives.

## 2. The working integration (all of this worked)

Everything below ran from `/home/kara/hrs-dogfood-20261017/transcript_archive.py`
— a consumer written **from `specs/python-interop.md` §4 alone**, importing none
of the repo's own modules, stdlib only (`json`, `subprocess`, `sqlite3`).

```python
# build once
cargo build --release --bins          # → <target>/release/hermes-rs-session-adapter

# then, from Python, exactly as the wire table documents it
proc = subprocess.Popen([adapter], stdin=PIPE, stdout=PIPE, text=True, bufsize=1)
proc.stdin.write(json.dumps({"id": 1, "method": "open",
                             "params": {"path": db, "create": True}}) + "\n")
reply = json.loads(proc.stdout.readline())          # {"id":1,"result":{...}}
```

Frames are one JSON object per line; `{"id","result"}` on success,
`{"id","error":{"error","cause","detail"}}` on failure. Implementing a client
from the spec took ~60 lines and no source reading — the wire contract is
genuinely implementable from the doc.

What worked end-to-end, at scale:

* **archive** — 200 sessions × 20 messages = **4,000 messages in 29.5 s**
  (136 msg/s; per-session median 0.137 s, p95 0.192 s) including
  `ensure_session` + appends + `end_session` + `checkpoint` + `backup`.
* **read-back through both processes** — a second adapter process re-read the
  same file; stdlib `sqlite3` independently read the same rows and
  `PRAGMA integrity_check = ok`.
* **structured errors** — append into a never-ensured session returns
  `AdapterError(error='SessionRowMissing', cause='session_row_missing')`, and
  the store stays usable afterwards.
* **lifecycle** — `close` + `shutdown` exit 0, no orphaned adapter, no `-wal`/`-shm`
  leftovers.
* **fresh machine** — on an ephemeral bunker agent the documented path
  (rustup → `cargo build`) worked and the repo's own interop proof printed all
  five markers there (`docs/evidence/dogfood-20261017/bunker-e2e.log`).

## 3. What broke, and what the user sees

### 3.1 Search is not trustworthy (`DF-HRS-104`, `DF-HRS-105`) — the headline finding

`search` is the operation a session store is *for*, and it silently returns
nothing for text that is in the store. Two independent mechanisms in
`sanitize_fts5_query`:

**Mechanism A — words starting/ending with `and`/`or`/`not` are truncated**
(`stock_sqlite.rs:1079-1092`, `starts_with`/`ends_with` on the whole query
instead of whole-token matching):

| user typed | what the store searched | hits | correct answer |
|---|---|---|---|
| `notes` | `es` | 0 | 1 |
| `nothing` | `hing` | 0 | 1 |
| `android` | `roid` | 0 | 1 |
| `order` | `der` | 0 | 1 |
| `command` | `comm` | 0 | 1 |
| `brand` | `br` | 0 | 1 |
| `the command` | `the comm` | 0 | 1 |

9 of 16 ordinary words in the probe matrix returned a silent zero
(`opbug.log`); `search "brand"` returned zero rows while the file's own
`messages_fts MATCH 'brand'` returned the row that contains it
(`wrong-answer.log`).

**Mechanism B — punctuation merges words into a token that cannot exist**
(`stock_sqlite.rs:1049-1054` deletes the specials with no separator,
`:1093-1120` then quotes the merged term):

| user typed (present in the store) | what was searched | hits |
|---|---|---|
| `src/main.rs` | phrase `"srcmain.rs"` | 0 |
| `cost=5.50` | `"cost5.50"` | 0 |
| `email a.b@example.com` | `email "a.bexample.com"` | 0 |
| `path C:\Users\kara` | `path CUserskara` | 0 |

19 of 31 realistic verbatim strings were unfindable while stdlib `LIKE` found
every one of them in the same row (`search-fidelity.log`). The zero carries **no
diagnostic**: `zzz-no-such-text-zzz` and `zzz/no-such/text` both answer 0 too,
and the non-stale path has no LIKE fallback (`like_fallback` at
`stock_sqlite.rs:1342` serves only the stale/corrupt arms).

**User-facing consequence:** "I know I wrote that, and the store says it isn't
there." That is the worst failure a memory product can have, and it is silent.

### 3.2 A fresh store lies about being fresh, and about being healthy

`open(create=True)` on a brand-new path answered
(`fresh-store-control.log`):

```json
{"created_new_store": false, "schema_version_before": null,
 "schema_version_after": 31,
 "degraded": ["fresh-store census mismatch after scaffold"]}
```

* `created_new_store` is **never assigned anywhere** in `src/` — the local is set
  at `:402`, the report field is not (`DF-HRS-106`). The Python client exposes it
  as `OpenReport.created_new_store`, so consumers branch on a constant `false`.
* the store's own census check fires on a healthy fresh store: the diff pins it
  to `idx_messages_platform_msg_id` and `idx_sessions_title_unique`, which exist
  **only in the census constant** and are never created (`DF-HRS-107`) — so a
  fresh install also carries a file with 2 fewer indexes than the census expects.

### 3.3 Repeating an operation a user must repeat

`backup` to the same destination fails on the second run
(`Io: backup destination exists`), because the destination is required not to
exist while the upstream snapshot this slice mirrors *pre-creates* it
(`DF-HRS-108`, `backup-second-run.log`). A nightly backup loop dies on night
two.

### 3.4 The Python-facing surface is smaller than the spec claims

`specs/python-interop.md` §3.1 says the boundary exposes "exactly the §6.1
operation set … `record_model_usage`", and §6.1 lists
`append_messages_batch` ("one transaction for the whole batch"). Both are now
on the wire as of DF-HRS-109. The client exposes these methods without claiming
drop-in compatibility.

### 3.5 A fresh user's documented install is red

`README.md` says: *"From a clean checkout, run: cargo build; cargo test."* On a
bare Debian bunker agent: `cargo build` rc=0 (25 s) and **`cargo test` rc=101 —
17 passed, 5 failed**, every failure `Os { code: 2, kind: NotFound }` from a
test that shells out to a `sqlite3` CLI the box does not have
(`DF-HRS-110`, `bunker-test.log`). The control host (same commit, has
`sqlite3 3.53.4`) is 29/29 green, so the class is machine-dependent — but the
documented path is what a fresh user follows, and it ends red with no named
prerequisite.

## 4. What costs time (measured — Step 2b)

| operation | number | verdict |
|---|---|---|
| adapter spawn → `hello` reply | 78 ms | fine |
| `open` existing store | ~10 ms | fine |
| `append_message` (one durable commit) | 7–10 ms median | fine per message |
| `search` on 4,000 messages | 0.7 ms median, 1.3 ms max | fast |
| archive 4,000 messages | 29.5 s (136 msg/s) | noticeable for bulk import; **root cause is the missing batch surface**, not a slow append (`DF-HRS-109`) |
| `cargo build` on a fresh box | 25 s (+16 s rustup) | fine |
| install footprint | ~1.9 GB (`.rustup` 1.5 G, `.cargo` 85 M, `target` 284 M) | expected for Rust |

Load average on the measuring host was 20–28 (16 cores), so wall-clock numbers
are pessimistic. No standalone `PERF-<n>` row was filed: nothing a user does
one-at-a-time is slow enough to feel; the only archivable number is the bulk
import, and its fix is the batch op (`DF-HRS-109`).

## 5. Integration verdict

**🟡 PROMISING-BUT-ROUGH.** The boundary itself is real and pleasant — a
60-line stdlib client from the wire table, structured errors, clean lifecycle,
durable file another SQLite stack can read. What is not trustworthy yet is
*what the store says*: `search` silently drops results for ordinary words and
for anything with punctuation; a fresh store reports itself unhealthy and
not-fresh; the documented smoke command fails on a clean machine. Fix the two
search mechanisms and the fresh-store report and this stops being rough.

## 6. Reproducing

```sh
cargo build --release --bins            # → <CARGO_TARGET_DIR>/release/hermes-rs-session-adapter
cd docs/evidence/dogfood-20261017
python3 transcript_archive.py.txt --mode archive --adapter <adapter> --db /tmp/x/state.db \
        --sessions 200 --msgs 20
python3 probe_opbug.py.txt <adapter>
python3 probe_fidelity.py.txt --adapter <adapter> --db /tmp/x/state.db --leg search
python3 probe_backup.py.txt <adapter>
```

Probe sources are archived as `.py.txt` beside their captured output so the
captures above can be regenerated verbatim.
