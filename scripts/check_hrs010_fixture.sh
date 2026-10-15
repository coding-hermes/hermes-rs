#!/usr/bin/env bash
# HRS-010 fixture check: the committed Python-produced fixture must be a
# healthy standard SQLite file whose rows match the committed manifest
# (tests/fixtures/hrs010/manifest.json) — verified with the system sqlite3
# binary, independent of both the pinned Python reference and the Rust
# store. F-2/F-10 stock-file compatibility leg at slice scale.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DB="$REPO/tests/fixtures/hrs010/state.db"
MANIFEST="$REPO/tests/fixtures/hrs010/manifest.json"

fail() { echo "FAIL: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "missing tool: $1"; }
need sqlite3
need jq

[ -f "$DB" ] || fail "fixture missing: $DB"
[ -f "$MANIFEST" ] || fail "manifest missing: $MANIFEST"

# 1. File health (stock SQLite).
integrity="$(sqlite3 "$DB" 'PRAGMA integrity_check;')"
[ "$integrity" = "ok" ] || fail "integrity_check: $integrity"
[ "$(sqlite3 "$DB" 'PRAGMA quick_check;')" = "ok" ] || fail "quick_check failed"

# 2. File-hash matches the committed manifest (no drift since production).
committed_sha="$(jq -r '.state_db_sha256' "$MANIFEST")"
actual_sha="$(sha256sum "$DB" | awk '{print $1}')"
[ "$committed_sha" = "$actual_sha" ] || fail "sha256 drift: manifest=$committed_sha actual=$actual_sha"

# 3. Schema version and object census match the manifest exactly (F-10 shape).
[ "$(sqlite3 "$DB" 'SELECT version FROM schema_version LIMIT 1;')" = "$(jq -r '.schema_version' "$MANIFEST")" ] \
  || fail "schema_version mismatch"
for typ in table index view trigger; do
  db_names="$(sqlite3 "$DB" "SELECT name FROM sqlite_master WHERE type='$typ' AND name NOT LIKE 'sqlite_%' ORDER BY name;" | jq -Rsc 'split("\n")[:-1] | sort')"
  mf_names="$(jq -c ".sqlite_master_census.$typ | sort" "$MANIFEST")"
  [ "$db_names" = "$mf_names" ] || fail "sqlite_master $typ census mismatch: db=$db_names manifest=$mf_names"
done

# 4. Row-level truth: messages, sessions, usage, FTS counts, generation.
check_eq() { # label actual expected
  [ "$2" = "$3" ] || fail "$1: actual=$2 expected=$3"
}
check_eq "messages count" \
  "$(sqlite3 "$DB" 'SELECT COUNT(*) FROM messages;')" "$(jq -r '.fts_counts.messages' "$MANIFEST")"
check_eq "messages_fts count" \
  "$(sqlite3 "$DB" 'SELECT COUNT(*) FROM messages_fts;')" "$(jq -r '.fts_counts.messages_fts' "$MANIFEST")"
check_eq "trigram count" \
  "$(sqlite3 "$DB" 'SELECT COUNT(*) FROM messages_fts_trigram;')" "$(jq -r '.fts_counts.messages_fts_trigram' "$MANIFEST")"

jq -c '.sessions | to_entries[]' "$MANIFEST" | while read -r entry; do
  sid="$(jq -r '.key' <<<"$entry")"
  count="$(sqlite3 "$DB" "SELECT COUNT(*) FROM messages WHERE session_id = '$sid' AND active = 1;")"
  expected="$(jq -r '.value.messages | length' <<<"$entry")"
  check_eq "session $sid message count" "$count" "$expected"
  first_content="$(sqlite3 "$DB" "SELECT content FROM messages WHERE session_id = '$sid' ORDER BY id LIMIT 1;")"
  manifest_first="$(jq -r '.value.messages[0].content' <<<"$entry")"
  check_eq "session $sid first message content" "$first_content" "$manifest_first"
  first_uid="$(sqlite3 "$DB" "SELECT message_uid FROM messages WHERE session_id = '$sid' ORDER BY id LIMIT 1;")"
  manifest_uid="$(jq -r '.value.messages[0].message_uid' <<<"$entry")"
  check_eq "session $sid first message_uid" "$first_uid" "$manifest_uid"
  reason="$(sqlite3 "$DB" "SELECT COALESCE(end_reason, '') FROM sessions WHERE id = '$sid';")"
  manifest_reason="$(jq -r '.value.row.end_reason // ""' <<<"$entry")"
  check_eq "session $sid end_reason" "$reason" "$manifest_reason"
  tokens="$(sqlite3 "$DB" "SELECT input_tokens FROM sessions WHERE id = '$sid';")"
  manifest_tokens="$(jq -r '.value.row.input_tokens' <<<"$entry")"
  check_eq "session $sid input_tokens" "$tokens" "$manifest_tokens"
done

# 5. Search truth through stock SQLite FTS5.
hits="$(sqlite3 "$DB" "SELECT rowid FROM messages_fts WHERE messages_fts MATCH 'unique_token_xyz' ORDER BY rowid;" | jq -Rsc 'split("\n")[:-1] | map(tonumber)')"
expected_hits="$(jq -c '[.search.unique_token_xyz[] | .[0]]' "$MANIFEST")"
check_eq "FTS hit ids for unique_token_xyz" "$hits" "$expected_hits"

# 6. Generation counter truth (never-GC surface read back verbatim).
gens="$(sqlite3 "$DB" 'SELECT source, session_key, generation FROM conversation_generations ORDER BY source, session_key;' | jq -Rsc 'split("\n")[:-1] | map(split("|") | {source: .[0], session_key: .[1], generation: (.[2] | tonumber)})')"
expected_gens="$(jq -c '.conversation_generations' "$MANIFEST")"
[ "$gens" = "$expected_gens" ] || fail "conversation_generations mismatch: $gens vs $expected_gens"

# 7. Usage rollup truth.
usage_count="$(sqlite3 "$DB" 'SELECT COUNT(*) FROM session_model_usage;')"
check_eq "session_model_usage rows" "$usage_count" "$(jq '.session_model_usage | length' "$MANIFEST")"

echo "OK: fixture matches manifest (integrity, census, rows, FTS, generations, usage)"
