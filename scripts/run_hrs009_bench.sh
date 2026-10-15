#!/bin/sh
# HRS-009 benchmark runner — executes on the IDLE benchmark host (master001).
# The kara agent box runs at load ~50 (documented I/O starvation) and is
# excluded by policy; numbers published from there would be noise.
#
# Usage (from a synced checkout on the bench host):
#   sh scripts/run_hrs009_bench.sh /path/to/repo [--ops N] [--sessions N]
#
# Runs BOTH arms (stock + turso) on identical datasets and stores raw JSON
# under benchmarks/results/. Refuses to run under load average > 4.
set -eu
REPO="${1:-.}"
shift || true
cd "$REPO"

LOAD=$(cut -d' ' -f1 /proc/loadavg)
case "$LOAD" in
  *[!0-9.]*) echo "refusing: unreadable loadavg" ; exit 2 ;;
esac
if [ "$(echo "$LOAD > 4" | bc 2>/dev/null || echo 1)" = "1" ]; then
  echo "refusing: load average $LOAD > 4 — benchmark host must be idle"
  exit 2
fi

mkdir -p benchmarks/results
CARGO=${CARGO:-$HOME/.cargo/bin/cargo}

echo "== building release binaries =="
"$CARGO" build --release --example hrs009_bench
"$CARGO" build --release --features turso --example hrs009_bench

echo "== stock arm =="
"$CARGO" run --release --example hrs009_bench -- \
  --backend stock --out benchmarks/results/hrs009-stock.json "$@"

echo "== turso arm =="
"$CARGO" run --release --features turso --example hrs009_bench -- \
  --backend turso --out benchmarks/results/hrs009-turso.json "$@"

echo "== results =="
ls -la benchmarks/results/
