# RELEASE-002 — the publish path is executed, not assumed

Row: `RELEASE-002` (P2) — the Release workflow had zero runs, ever, and its
only entry points were a `v*` tag push or a dispatch that required an existing
tag, so the first execution of the whole path would have been the first
public tag.
Trace: `ch:trace RELEASE-002 | spec=specs/ROADMAP.md | evidence=docs/evidence/release-002/`

Head of every receipt below: `44240780e56016ac3e8f7663ad72300a1cfb2eaf`.

## What changed

`.github/workflows/release.yml` now has two deliberately different entry
points, and `docs/releases.md` documents both:

| Path | Trigger | Checks out | Publishes |
|---|---|---|---|
| publish | `push` of `v*`, or `workflow_dispatch` with `tag` and `dry_run` false | the tag | yes, after `verify` |
| rehearsal | `workflow_dispatch` with `version` (for example `0.1.0` or `v0.1.0`) and `dry_run` true | the dispatched commit (`github.sha`) | never |

Structural changes, all of them needed to make the rehearsal mean something:

- `prepare` resolves a **ref** separately from the **tag** — a dry run checks
  out the dispatched commit, a publish run checks out the tag.
- The version is an explicit input on the rehearsal path instead of being
  derived from `GITHUB_REF_NAME`, and a `tag`/`version` pair that disagrees
  fails with an explicit message.
- A publish dispatch with no tag now says so, instead of silently falling back
  to the ref name.
- "The dispatched publish tag must exist on origin" is a new check that only
  runs on the publish dispatch path.
- `SHA256SUMS` assembly and `sha256sum -c` moved out of `publish` into a new
  `verify` job that every path runs. Previously the checksum step lived inside
  the publish job and was therefore skipped whenever publish was skipped — a
  dry run could not have verified anything.
- `publish` is gated on `needs.prepare.outputs.dry_run != 'true'`; on the
  rehearsal path no Release object and no release asset is created.
- `rehearse_undated_changelog` exists only to let a rehearsal run before the
  CHANGELOG section is stamped. It is honoured only with `dry_run` true, it
  waives exactly the dated-section check and emits a `::warning::`, and it
  cannot waive that check on a publish path (proven by the third receipt).

## Receipts

| # | Run | Conclusion | What it proves |
|---|---|---|---|
| 1 | [43935640646](https://github.com/coding-hermes/hermes-rs/actions/runs/43935640646) | success | the rehearsal path runs `prepare` → both `build` matrix legs → `verify`, and stops: `Publish the GitHub Release` is skipped |
| 2 | [43935679198](https://github.com/coding-hermes/hermes-rs/actions/runs/43935679198) | failure (intended) | a version that disagrees with `Cargo.toml` fails in `prepare` with `requested version '9.9.9' does not match Cargo.toml [package] version '0.1.0'` |
| 3 | [43935717716](https://github.com/coding-hermes/hermes-rs/actions/runs/43935717716) | failure (intended) | a version with no dated `CHANGELOG.md` section fails the preflight with `CHANGELOG.md has no dated section for 0.1.0` |

Runs 2 and 3 are deliberate negative controls, not broken CI. They are red on
the dashboard because the row asked for the failing paths to be proven by run
id, and they fail in `prepare` before any build or publish job starts.

### Run 1 in detail

- `prepare`: resolve (dry run, version `0.1.0`, `rehearse_undated_changelog`
  true), checkout of `4424078`, main-line check, manifest version match,
  CHANGELOG section waived with a warning. The "publish tag must exist" step is
  correctly **skipped** on this path.
- `Build x86_64-unknown-linux-gnu`: success, including the host smoke step,
  which printed exactly `hermes-rs 0.1.0` and `hermes-rs/session-adapter/1`.
- `Build aarch64-unknown-linux-gnu`: success; `dist/hermes-rs-v0.1.0-aarch64-unknown-linux-gnu`
  is a real `ELF 64-bit LSB pie executable, ARM aarch64`.
- `verify`: all four expected names present —
  `hermes-rs-v0.1.0-{x86_64,aarch64}-unknown-linux-gnu` and
  `hermes-rs-session-adapter-v0.1.0-{x86_64,aarch64}-unknown-linux-gnu` — then
  `sha256sum -c SHA256SUMS` reports `OK` for every one of them and the manifest
  is uploaded as an artifact.
- `Publish the GitHub Release`: skipped.

The run produced three artifacts: `release-x86_64-unknown-linux-gnu`,
`release-aarch64-unknown-linux-gnu`, `release-SHA256SUMS` (369 B manifest).

## The defect the rehearsal found

The first hosted rehearsal ([43933406261](https://github.com/coding-hermes/hermes-rs/actions/runs/43933406261))
failed on the aarch64 leg, and the fault was in the workflow:

```
libsqlite3-sys@0.38.2: /usr/include/stdint.h:26:10: fatal error:
bits/libc-header-start.h: No such file or directory
error: failed to run custom build command for `libsqlite3-sys v0.38.2`
```

`libc6-dev-arm64-cross` is only a **Recommends** of `gcc-aarch64-linux-gnu`
(`apt-cache depends gcc-aarch64-linux-gnu`), and the install step ran with
`--no-install-recommends`, so the runner had the cross gcc without the target's
libc headers; cc-rs then compiled the bundled SQLite C sources against host
headers. The same build succeeds on the dev host, which happens to have that
package installed — which is exactly why the earlier local aarch64 build proof
(RELEASE-002's own reasoning, and the readiness sweep's) covered the toolchain
but not this dependency list. `4424078` names the package explicitly with the
reason in a comment, and run 1 above is the proof that the leg now builds.

This is the value of the row: it converted a "should work" into a measured
failure and a fix, without a tag ever existing.

## Invariants after all three runs

- `git ls-remote --tags origin` → empty; `git tag -l` → empty.
- `gh release list` → empty.
- No artifact is a release asset; the three artifacts are ordinary workflow
  artifacts on run 1.

## Reproducing

```
gh workflow run release.yml --ref main \
  -f dry_run=true -f version=0.1.0 -f rehearse_undated_changelog=true   # green rehearsal
gh workflow run release.yml --ref main -f dry_run=true -f version=9.9.9 # preflight: version vs Cargo.toml
gh workflow run release.yml --ref main -f dry_run=true -f version=0.1.0 # preflight: dated CHANGELOG
```

The same `prepare` script was also exercised locally before any push — the
resolve step extracted from the workflow file itself and run under ten
event/input combinations (`resolve-matrix.log` in this directory): push tag,
bad tag, publish dispatch, publish dispatch with no tag, dry run by version,
dry run by `v`-prefixed version, tag/version mismatch, tag/version match, dry
run with no version, malformed version. Each produced either the intended
`ref`/`tag`/`version`/`dry_run` or the intended refusal.
