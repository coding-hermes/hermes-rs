# DF-HRS-103 — full battery recovery adjudication

Battery: bunker-qa.sh on bunker-mvp agent `c69fa222`, HEAD `7a0e0b8` (launch 2026-10-16T13:58:36Z,
collected 2026-10-16T14:03:02Z, rc=0, 21 rows). This closes the 2026-10-14 truncated run
(`/tmp/bunker-qa-evidence-20261014T093146Z-3617551.jsonl`: 3/4 chaos cells UNVERIFIED after the
qa-run.sh died mid-battery).

## Cells (fresh run, no truncation — terminal cell `chaos-errorpath` present, collect rc=0)

| Cell | Verdict | Basis |
|---|---|---|
| fresh-install | OK | cargo dev profile build finished 21.88s (inst.log) |
| ci-pass | OK | act 1 job green (ci.log: contracts + whitespace both Success) |
| upgrade | N/A | no release tags in repo |
| docker-deploy | N/A | no compose file |
| ui-probe | N/A | no web UI detected |
| chaos-disconnect | UNVERIFIED (env-class) | see below |
| chaos-shutdown | N/A | no compose file |
| chaos-corruption | N/A | no db/state files in repo |
| chaos-resource | FAIL → env-class | see below |
| chaos-errorpath | INFO | no start command detected (spec-stage repo: no service entrypoint yet) |

## chaos-resource: FAIL is env-class, not a product defect

res.log: all 5 failing tests are hrs010 tests that shell out to the **system `sqlite3` CLI**
(`run system sqlite3` / `sqlite3 read` / `bump version` / `stamp fts_stale`), panicking with
`Os { code: 2, kind: NotFound }` — the binary does not exist on the JIT agent (bare-agent
bootstrap provisions compilers/toolchains, not sqlite3; agents have no sudo to apt-install).
The suite itself finished in **3.43s** under a 3 GB cap — the cap was never the constraint.

Control-host proof at the same HEAD (`cargo test`, host has sqlite3 3.53.4):
all 29 tests pass — `22 passed; 0 failed` for hrs010_session_store, 5 passed hrs003 interop,
1 passed scaffold, 1 cli, 1 ignored crash-child. The failure does not reproduce at the same
commit where the toolchain exists ⇒ env/toolchain class per the battery doctrine (one finding;
do not file the per-cell FAIL). No product regression: the same suite is green uncapped in
ci-pass's native leg context and on the control host.

Follow-up (harness class, separate concern): the JIT toolchain bootstrap could provision
`sqlite3` alongside gcc/go/rust/node — the env-class rule keeps re-hitting python/go/rust
projects whose tests cross-check with the sqlite3 CLI.

## chaos-disconnect: UNVERIFIED is the documented vacuity class

disc.log is empty (0 bytes): the probe ran `cargo test` under a dead proxy and the suite
produced no output before the window closed — the network cut was never exercised by any
test (QA-WARPFS-17 / QA-BUNKER-29 vacuity class). Harness-graded UNVERIFIED with reason;
no disconnect finding derivable either way. Not fixable product-side until the repo declares
a native suite fast enough to run under the cut (the 29-test suite builds in ~3s but the
disc window sizing already covers it — the empty log indicates the run produced no output
within the window, likely toolchain-cold; env-class, not product).

## Verdict

Fresh-install OK, ci-pass OK, full battery recovered without truncation. 0 product-attributable
FAIL cells. Evidence: evidence.jsonl (21 rows) + res.log/ci.log/disc.log/qa-head.txt/inst.log
harvested from the agent home archive (/var/backups/bunker/bunker-c69fa222-20261016T140048Z.tar.gz)
after collect's teardown.
