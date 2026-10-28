#!/usr/bin/env bash
# Repo-owned chaos-errorpath probe for the hermes-rs session adapter
# (board row DF-HRS-121). Invoked by the bunker QA battery from the repo
# root with a 120 s budget; harness exit-code contract:
#   0 = OK   1 = FAIL (product misbehavior)   2 = INFO (environment gap)
# The FIRST stdout line is the cell detail — keep it one line and
# machine-greppable. Probe ownership: .qa/chaos-probes/ in this repo.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# Same target-dir resolution cargo uses: an operator-provided
# CARGO_TARGET_DIR (e.g. a scratch-disk redirect on a loaded host) wins;
# the default battery environment gets the documented repo-relative
# target/debug path.
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
ADAPTER="$TARGET_DIR/debug/hermes-rs-session-adapter"

ARM_RC=0
STDOUT_F="$(mktemp)"
STDERR_F="$(mktemp)"
trap 'rm -f "$STDOUT_F" "$STDERR_F"' EXIT

# capture <stdin-line|""> <cmd...> — runs the command, records its exit
# code and captures stdout/stderr to files for the detail report.
capture() {
    local data="$1"
    shift
    if [[ -n "$data" ]]; then
        printf '%s\n' "$data" | "$@" >"$STDOUT_F" 2>"$STDERR_F"
    else
        "$@" >"$STDOUT_F" 2>"$STDERR_F" </dev/null
    fi
    ARM_RC=$?
}

first_stderr() { head -n 1 "$STDERR_F"; }
stdout_all() { cat "$STDOUT_F"; }

detail() { printf '%s\n' "$*"; }

# ---- Step 1: build (environment gate; failure is a gap, not a red) ----
if ! (cd "$REPO_ROOT" && cargo build --bin hermes-rs-session-adapter --quiet); then
    echo "ERRORPATH_PROBE_INFO cargo build failed for hermes-rs-session-adapter (environment gap)"
    exit 2
fi
if [[ ! -x "$ADAPTER" ]]; then
    echo "ERRORPATH_PROBE_INFO adapter binary missing after build: $ADAPTER"
    exit 2
fi

fail() { # fail <arm> <expected> <actual>
    detail "arm$1: EXPECTED $2, GOT $3"
    exit 1
}

# ---- Arm A: protocol identity (control) ----
capture "" "$ADAPTER" --protocol
A_RC=$ARM_RC
A_STDOUT="$(stdout_all)"
[[ $A_RC -eq 0 ]] || fail A "exit 0" "exit $A_RC"
[[ "$A_STDOUT" == "hermes-rs/session-adapter/1" ]] \
    || fail A "stdout 'hermes-rs/session-adapter/1'" "stdout '$A_STDOUT'"

# ---- Arm B: unknown argument ----
capture "" "$ADAPTER" stray-arg
B_RC=$ARM_RC
B_ERR="$(first_stderr)"
[[ $B_RC -eq 2 ]] || fail B "exit 2" "exit $B_RC"
grep -q "unknown argument: stray-arg" "$STDERR_F" \
    || fail B "stderr 'unknown argument: stray-arg'" "stderr '$B_ERR'"

# ---- Arm C: malformed (non-JSON) stdin frame; distinct from unknown ----
# method, which fail-closes with exit 6 — this arm feeds non-JSON (rc 4).
capture '{not json' "$ADAPTER"
C_RC=$ARM_RC
C_ERR="$(first_stderr)"
[[ $C_RC -eq 4 ]] || fail C "exit 4" "exit $C_RC"
grep -q "malformed request frame" "$STDERR_F" \
    || fail C "stderr 'malformed request frame'" "stderr '$C_ERR'"

# ---- Arm D: valid control frame (hello needs no open store) ----
# Frame shape per scripts/hrs003_client.py _call: {"id": N, "method": "..."}.
capture '{"id": 1, "method": "hello"}' "$ADAPTER"
D_RC=$ARM_RC
D_STDOUT="$(stdout_all)"
[[ $D_RC -eq 0 ]] || fail D "exit 0" "exit $D_RC"
grep -q '"result"' "$STDOUT_F" || fail D "stdout carrying result" "stdout '$D_STDOUT'"
grep -q '"error"' "$STDOUT_F" && fail D "stdout without error" "stdout '$D_STDOUT'"

# ---- Report: FIRST line is the machine-greppable cell detail ----
echo "ERRORPATH_PROBE_OK protocol_rc=0 protocol=hermes-rs/session-adapter/1 unknown_arg_rc=2 malformed_rc=4 valid_frame_rc=0"
detail "armA: $ADAPTER --protocol | rc=$A_RC stdout='$A_STDOUT'"
detail "armB: $ADAPTER stray-arg | rc=$B_RC stderr[1]='$B_ERR'"
detail "armC: printf '{not json' | $ADAPTER ; rc=$C_RC stderr[1]='$C_ERR'"
detail "armD: printf '{\"id\": 1, \"method\": \"hello\"}' | $ADAPTER ; rc=$D_RC stdout[1]='$(printf '%s' "$D_STDOUT" | head -c 120)'"
exit 0
