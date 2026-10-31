# Hermes RS dogfood run 7 — 2026-10-31

## Promise

The repository documents a Rust CLI and a stock-SQLite session adapter. Earlier runs covered the adapter persistence workflow and fresh-machine probes. This run exercised the remaining standalone CLI instruction in `skills/hermes-rs-usage/SKILL.md` and attempted the mandatory ephemeral install leg.

## Target

Canonical checkout: `/home/kara/hermes-rs`
HEAD: `5c6e7d06c7c72123c8a9023a67d247b60c89825f` (equal to `origin/main` before write-back)

## Real-use result

The literal usage-skill command was:

```text
env CARGO_TARGET_DIR=<isolated> cargo run --quiet
```

It returned rc=101 before starting the product:

```text
error: `cargo run` could not determine which binary to run. Use the `--bin` option to specify a binary, or the `default-run` manifest key.
available binaries: hermes-rs, hermes-rs-session-adapter
```

The corrected control selected the binary:

```text
env CARGO_TARGET_DIR=<isolated> cargo run --quiet --bin hermes-rs
```

It returned rc=0 and printed `Hermes RS scaffold: no agent functionality implemented`. The adapter protocol control returned `hermes-rs/session-adapter/1`, rc=0. The existing `tests/cli.rs` passed 1/1. The binary accepts `--help` and an unknown argument but prints the same scaffold status line, so those flags do not provide normal help or argument validation.

Finding DF-HRS-122 records the broken documented command as observed at the measured HEAD. A sibling lane subsequently landed the documented-command fix in `1e1523e`; this run did not re-probe that post-fix tree. No product code was changed by this lane.

## Performance

The failed bare command is an availability/usability result, not a successful-operation timing. The completed CLI control took 0.09 s wall cold and 0.08 s warm using an isolated Cargo target. The adapter protocol build/run took 19.29 s wall because it compiled the isolated target. No completed operation was slow enough to justify profiling or a PERF row.

## Install leg

Status: `SKIPPED-install-bunker`.

The prescribed `bunker-qa.sh launch` path was attempted on `bunker-mvp`. Control-plane listing was reachable and showed no agents, but spawn failed at rootless-install while enabling linger:

```text
Could not enable linger: Connection timed out
root cause: agent ssh port 22 AND port 22 unreachable on bunker-mvp - server/network down, not a docker fault
```

A direct SSH probe to `bunker-mvp` also timed out. The helper produced one evidence row and no agent id. No install PASS is claimed, and no credential, permission, repository visibility, or product code was changed.

Finding DF-HRS-123 records the explicit skipped install boundary.

## Verdict

PROMISING-BUT-ROUGH. The implemented adapter workflow remains evidenced by prior runs. This run found a first-command documentation/usability failure in the usage skill, and the fresh-machine install leg remains unverified because the bunker spawn prerequisite was unavailable.

## Evidence

- `docs/evidence/dogfood-20261031/run7-transcript.txt`
- `docs/evidence/dogfood-20261031/install-evidence.jsonl`
- `docs/evidence/dogfood-20261031/run7-diagnostics.txt`
- Scratch transcript SHA-256: `b24eb4f0556e8d18334f923359e27a56d750ae3788a8d165d5a5b5898ff1ec52`
- Scratch install evidence SHA-256: `0cbd17c2f762b391ab6ce5300f81a9641ec79c405607c0c76ba560f5acf2c315`
