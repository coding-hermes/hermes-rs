#!/usr/bin/env bash
# Track an upstream Hermes Agent release into a deterministic snapshot
# (HRS-004). See scripts/build_upstream_snapshot.py for the fetch and
# snapshot contract; this wrapper is the stable CLI surface.
#
# Usage:
#   scripts/track_upstream_release.sh              # latest upstream release
#   scripts/track_upstream_release.sh v0.22.2      # explicit tag
#   scripts/track_upstream_release.sh v0.22.2 --prev v0.22.1
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

exec python3 "${SCRIPT_DIR}/build_upstream_snapshot.py" "$@"
