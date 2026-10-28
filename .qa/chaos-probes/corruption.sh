#!/usr/bin/env bash
# DF-HRS-120 — repo-owned chaos-corruption probe for the runtime SQLite state store.
#
# Harness contract (battery semantics): run from the REPO ROOT with a 120s timeout,
# stdout+stderr captured. Exit 0 = OK; 1 = FAIL (product misbehavior); 2 = INFO
# (environment gap, probe skipped). The FIRST stdout line becomes the cell detail —
# keep it compact and machine-greppable.
set -u

PROBE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$PROBE_DIR/../.." && pwd)"
cd "$REPO_ROOT"

# Scratch under the agent HOME (brief step 1); trap-cleanup on exit.
SCRATCH="$(mktemp -d "${HOME}/.hrscorr-XXXXXXXX")"
cleanup() { rm -rf "$SCRATCH"; }
trap cleanup EXIT

detail() { printf '  %s\n' "$*"; }

# 2. Environment preconditions: a missing input is an environment gap (exit 2),
#    never a product FAIL.
for f in scripts/hrs003_e2e_proof.py scripts/hrs003_client.py Cargo.toml; do
  if [ ! -f "$f" ]; then
    echo "CORRUPTION_PROBE_INFO repo file missing: $f (run from the repo root)"
    exit 2
  fi
done
command -v cargo >/dev/null 2>&1 || { echo "CORRUPTION_PROBE_INFO cargo not on PATH"; exit 2; }
command -v python3 >/dev/null 2>&1 || { echo "CORRUPTION_PROBE_INFO python3 not on PATH"; exit 2; }

if ! cargo build --bin hermes-rs-session-adapter --quiet >"$SCRATCH/build.log" 2>&1; then
  echo "CORRUPTION_PROBE_INFO cargo build failed (environment gap, not graded FAIL)"
  sed -n '1,6p' "$SCRATCH/build.log"
  exit 2
fi
# Locate the built adapter: repo-relative target/ first, then cargo's own
# target-directory resolution (CARGO_TARGET_DIR may redirect the build).
ADAPTER="$REPO_ROOT/target/debug/hermes-rs-session-adapter"
if [ ! -x "$ADAPTER" ]; then
  CARGO_TARGET_ROOT="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -n1)"
  if [ -n "$CARGO_TARGET_ROOT" ] && [ -x "$CARGO_TARGET_ROOT/debug/hermes-rs-session-adapter" ]; then
    ADAPTER="$CARGO_TARGET_ROOT/debug/hermes-rs-session-adapter"
  fi
fi
if [ ! -x "$ADAPTER" ]; then
  echo "CORRUPTION_PROBE_INFO adapter binary missing after build: target/debug/hermes-rs-session-adapter"
  exit 2
fi

# 3. Documented e2e proof must pass and carry both markers, else product FAIL.
python3 scripts/hrs003_e2e_proof.py --adapter "$ADAPTER" >"$SCRATCH/proof.out" 2>&1
proof_rc=$?
if [ "$proof_rc" -ne 0 ]; then
  echo "FAIL_HRS003_E2E_PROOF hrs003_e2e_proof.py rc=$proof_rc (expected 0)"
  tail -n 8 "$SCRATCH/proof.out"
  exit 1
fi
if ! grep -q "ROUNDTRIP_OK" "$SCRATCH/proof.out"; then
  echo "FAIL_ROUNDTRIP_OK marker missing from hrs003_e2e_proof.py output"
  tail -n 8 "$SCRATCH/proof.out"
  exit 1
fi
if ! grep -q "SQLITE_SCHEMA_VERSION=31" "$SCRATCH/proof.out"; then
  echo "FAIL_SQLITE_SCHEMA_VERSION marker missing (expected SQLITE_SCHEMA_VERSION=31, SCHEMA_VERSION_KNOWN src/persistence/api.rs)"
  tail -n 8 "$SCRATCH/proof.out"
  exit 1
fi

# 4-7. Client-driven second store + clean/corruption arms live in probe_state.py.
#      Its first stdout line becomes this probe's first stdout line.
python3 "$PROBE_DIR/probe_state.py" "$ADAPTER" "$SCRATCH" "$REPO_ROOT"
probe_rc=$?
if [ "$probe_rc" -eq 0 ]; then
  detail "cmd: cargo build --bin hermes-rs-session-adapter --quiet rc=0"
  detail "cmd: python3 scripts/hrs003_e2e_proof.py --adapter target/debug/hermes-rs-session-adapter rc=0"
fi
exit "$probe_rc"
