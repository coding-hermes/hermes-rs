#!/usr/bin/env bash
#
# scripts/release.sh — the repository-owned release driver for hermes-rs.
#
# hermes-rs is an independent Rust project. It is not an official Nous Research
# release and it is not a drop-in replacement for Hermes Agent. This script
# stamps a release; it makes no compatibility or performance claim.
#
# What this script does NOT do: it never pushes anything, never creates a GitHub
# Release object, and never publishes an artifact. Publishing is a separate,
# tag-triggered step (.github/workflows/release.yml) that CI runs only after an
# operator pushes the tag this script creates. See docs/releases.md.
#
# Usage:
#
#   scripts/release.sh --dry-run
#       Print the complete release plan: the version stamp and its consistency
#       checks, the exact CHANGELOG section that a cut would write, the
#       artifact list, and the SHA256SUMS plan. Exit 0. Creates no tag and no
#       commit, and writes no file inside the repository (the CHANGELOG
#       transform is exercised into a temporary directory and discarded).
#
#   scripts/release.sh --confirm-readiness <readiness-row-id>
#       A real cut: re-check cleanliness and tag absence, run the local gates,
#       build the host artifacts, render the dated CHANGELOG section from
#       [Unreleased], write release notes and SHA256SUMS under
#       target/release-artifacts/<tag>/, commit CHANGELOG.md, and create an
#       annotated tag locally. It does not push — pushing the tag is the
#       operator's separate step.
#
#   scripts/release.sh
#       Refuses with an explicit message. See "Safety on a clean tree" below.
#
# Safety on a clean tree with the current 0.1.0 stamp: this driver REFUSES
# (exit 2) instead of silently no-opping. A cut requires a named readiness row
# that returned GO, passed as --confirm-readiness <row>. The driver can prove
# the mechanical preconditions (clean tree, absent tag, unreleased version,
# non-empty [Unreleased]) but it cannot decide readiness — that decision lives
# with the readiness sweep row. Use --dry-run to inspect the plan for free.
#
# Refusals (exit 2), each with an explicit message on stderr:
#   - the working tree is dirty (tracked or untracked changes);
#   - HEAD is not on main (releases are cut from main);
#   - the target tag already exists locally;
#   - the target tag already exists on origin;
#   - the origin tag check could not be completed (fail closed, never assume);
#   - Cargo.toml's version already has a dated CHANGELOG section;
#   - the [Unreleased] section is empty (nothing to release);
#   - a real cut was requested without --confirm-readiness <row>.
#
# Failures (exit 1): version-stamp drift between Cargo.toml and Cargo.lock, a
# malformed CHANGELOG, a failing gate, or a failing artifact build.
#
# Exit codes: 0 = dry run printed, or the cut completed; 1 = a check, gate or
# build failed; 2 = refused before making any change.

set -euo pipefail

SCRIPT_NAME="release.sh"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

CARGO_TOML="$ROOT/Cargo.toml"
CARGO_LOCK="$ROOT/Cargo.lock"
CHANGELOG="$ROOT/CHANGELOG.md"

MODE="cut"
READINESS=""

# Temporary paths live outside the repository and are removed on exit. They are
# global so the EXIT trap can see them (a `local` is gone by trap time).
TMP_DIR=""
TMP_CHANGELOG=""

cleanup_temporary_paths() {
	if [ -n "$TMP_CHANGELOG" ]; then
		rm -f -- "$TMP_CHANGELOG"
	fi
	if [ -n "$TMP_DIR" ]; then
		rm -rf -- "$TMP_DIR"
	fi
	return 0
}

trap cleanup_temporary_paths EXIT

# Binaries this repository ships. Both are built for the host target by a real
# cut; the tag-triggered workflow builds them for the two published targets.
readonly BINARIES=(hermes-rs hermes-rs-session-adapter)

usage() {
	cat <<'EOF'
scripts/release.sh — release driver for hermes-rs

  --dry-run                       print the full release plan; change nothing
  --confirm-readiness <row-id>    perform a real cut (local commit + local tag)
  -h, --help                      print this help

A real cut refuses unless <row-id> names the readiness sweep row that returned
GO. This driver publishes nothing: pushing the tag is a separate operator step
that runs .github/workflows/release.yml. See docs/releases.md.

Exit codes: 0 = dry run printed or cut completed; 1 = check/gate/build failed;
2 = refused before making any change.
EOF
}

refuse() {
	printf '%s: REFUSED: %s\n' "$SCRIPT_NAME" "$*" >&2
	exit 2
}

fail() {
	printf '%s: FAILED: %s\n' "$SCRIPT_NAME" "$*" >&2
	exit 1
}

info() {
	printf '%s\n' "$*"
}

# ---------------------------------------------------------------------------
# Repository facts
# ---------------------------------------------------------------------------

read_manifest_version() {
	awk '
		/^\[package\]$/ { in_package = 1; next }
		/^\[/ { in_package = 0 }
		in_package && $1 == "version" {
			v = $3
			gsub(/"/, "", v)
			print v
			exit
		}
	' "$CARGO_TOML"
}

read_lock_version() {
	awk '
		/^\[\[package\]\]$/ { name = "" }
		$1 == "name" {
			n = $3
			gsub(/"/, "", n)
			name = n
		}
		$1 == "version" && name == "hermes-rs" {
			v = $3
			gsub(/"/, "", v)
			print v
			exit
		}
	' "$CARGO_LOCK"
}

worktree_state() {
	local state
	state="$(git -C "$ROOT" status --porcelain --untracked-files=normal)"
	if [ -z "$state" ]; then
		printf 'clean\n'
	else
		printf 'dirty\n'
	fi
}

local_tag_exists() {
	git -C "$ROOT" rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null 2>&1
}

# Echoes the dated heading line when Cargo.toml's version already has one.
released_section_line() {
	local escaped
	escaped="${VERSION//./\\.}"
	grep -E "^## \[${escaped}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$" "$CHANGELOG" || true
}

# Everything between "## [Unreleased]" and the next level-2 heading, with
# leading and trailing blank lines removed.
unreleased_body() {
	awk '
		/^## \[Unreleased\]$/ { inside = 1; next }
		inside && /^## / { exit }
		inside { print }
	' "$CHANGELOG" | sed -e '/./,$!d' | tac | sed -e '/./,$!d' | tac
}

# The complete CHANGELOG.md a cut would write, on stdout. $1 = release date.
# [Unreleased] stays at the top as an empty section; the old body moves under a
# dated heading; everything from the following level-2 heading onward (a
# previously released section, if any) is preserved verbatim.
render_next_changelog() {
	local date="$1" head rest
	head="$(sed -n '1,/^## \[Unreleased\]$/p' "$CHANGELOG")"
	rest="$(awk '/^## \[Unreleased\]$/{seen=1; next} seen && /^## /{print_all=1} print_all' "$CHANGELOG")"
	printf '%s\n\n' "$head"
	printf '## [%s] - %s\n\n%s\n' "$VERSION" "$date" "$UNRELEASED_BODY"
	if [ -n "$rest" ]; then
		printf '\n%s\n' "$rest"
	fi
}

artifact_name() {
	printf '%s-%s-%s' "$1" "$TAG" "$HOST_TARGET"
}

run_in_root() {
	( cd "$ROOT" && "$@" )
}

# ---------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------

while [ $# -gt 0 ]; do
	case "$1" in
		--dry-run)
			MODE="dry-run"
			shift
			;;
		--confirm-readiness)
			[ $# -ge 2 ] || refuse "--confirm-readiness needs a readiness board row id"
			READINESS="$2"
			shift 2
			;;
		--confirm-readiness=*)
			READINESS="${1#*=}"
			shift
			;;
		-h | --help)
			usage
			exit 0
			;;
		*)
			refuse "unknown argument: $1 (try --help)"
			;;
	esac
done

if [ "$MODE" = "dry-run" ] && [ -n "$READINESS" ]; then
	refuse "--dry-run and --confirm-readiness are mutually exclusive"
fi

# ---------------------------------------------------------------------------
# Preconditions that must hold for every mode
# ---------------------------------------------------------------------------

command -v git >/dev/null 2>&1 || fail "git is not on PATH"
git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1 ||
	fail "$ROOT is not inside a git work tree"
command -v rustc >/dev/null 2>&1 ||
	fail "rustc is not on PATH (needed to name host artifacts)"
command -v cargo >/dev/null 2>&1 || fail "cargo is not on PATH"

[ -f "$CARGO_TOML" ] || fail "missing $CARGO_TOML"
[ -f "$CARGO_LOCK" ] || fail "missing $CARGO_LOCK"
[ -f "$CHANGELOG" ] || fail "missing $CHANGELOG (the driver has nothing to stamp)"

VERSION="$(read_manifest_version)"
[ -n "$VERSION" ] || fail "could not read [package] version from Cargo.toml"
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$ ]]; then
	fail "Cargo.toml [package] version '$VERSION' is not a semver version"
fi

TAG="v$VERSION"
LOCK_VERSION="$(read_lock_version)"
if [ "$LOCK_VERSION" != "$VERSION" ]; then
	fail "version stamp drift: Cargo.toml says '$VERSION' but Cargo.lock says '${LOCK_VERSION:-<absent>}' — reconcile before cutting"
fi

HOST_TARGET="$(rustc -vV | awk '/^host: /{print $2}')"
[ -n "$HOST_TARGET" ] || fail "could not determine the host target via rustc -vV"
TODAY="$(date -u +%Y-%m-%d)"

if ! grep -Eq '^## \[Unreleased\]$' "$CHANGELOG"; then
	fail "CHANGELOG.md has no '## [Unreleased]' heading"
fi
UNRELEASED_BODY="$(unreleased_body)"
if printf '%s\n' "$UNRELEASED_BODY" | grep -Eq '^\[[^]]+\]:'; then
	fail "the [Unreleased] body contains a link-reference line ([name]: ...); the driver preserves only level-2 sections, so drop the footer before cutting"
fi

RELEASED_SECTION="$(released_section_line)"
WORKTREE="$(worktree_state)"
BRANCH="$(git -C "$ROOT" branch --show-current)"
if local_tag_exists; then
	LOCAL_TAG="present"
else
	LOCAL_TAG="absent"
fi

# ---------------------------------------------------------------------------
# Dry run: print the plan, change nothing
# ---------------------------------------------------------------------------

print_dry_run() {
	local tmpdir rendered rendered_sha synthetic_note

	TMP_DIR="$(mktemp -d)"
	tmpdir="$TMP_DIR"

	info "hermes-rs release driver — DRY RUN"
	info ""
	info "Nothing is created by this run: no tag, no commit, and no file inside the"
	info "repository. The CHANGELOG transform below is exercised into a temporary"
	info "directory and discarded. No artifact is built."
	info ""
	info "Repository facts"
	info "  repository         : $ROOT"
	info "  version (manifest) : $VERSION   (Cargo.toml [package] version)"
	info "  version (lock)     : $LOCK_VERSION   (matches the manifest)"
	info "  tag                : $TAG"
	info "  branch             : ${BRANCH:-<detached HEAD>}"
	info "  working tree       : $WORKTREE"
	info "  tag locally        : $LOCAL_TAG"
	info "  already released   : ${RELEASED_SECTION:-no dated section for $VERSION}"
	info "  host target        : $HOST_TARGET (rustc $(rustc --version | awk '{print $2}'))"
	info "  release date stamp : $TODAY (UTC, evaluated at cut time)"
	info "  [Unreleased] lines : $(printf '%s\n' "$UNRELEASED_BODY" | grep -c . || true)"
	info ""

	info "Plan — a real cut (--confirm-readiness <row>) runs exactly these steps,"
	info "in this order, and stops at the first refusal or failure:"
	info "  0. refuse unless: tree clean, HEAD on main, tag $TAG absent locally and"
	info "     on origin, no dated CHANGELOG section for $VERSION, [Unreleased]"
	info "     non-empty, --confirm-readiness names the readiness row that returned GO"
	info "  1. version stamp    verify Cargo.toml [package] version == Cargo.lock"
	info "                      ('$VERSION'); the driver never invents a version bump"
	info "  2. gates            cargo fmt --all -- --check"
	info "                      cargo clippy --workspace --all-targets --all-features -- -D warnings"
	info "                      cargo test --workspace --all-features"
	info "  3. artifacts        cargo build --release --locked --target $HOST_TARGET"
	info "                        --bin hermes-rs --bin hermes-rs-session-adapter"
	info "                      -> target/release-artifacts/$TAG/$(artifact_name hermes-rs)"
	info "                      -> target/release-artifacts/$TAG/$(artifact_name hermes-rs-session-adapter)"
	info "  4. CHANGELOG        write the dated section shown below into CHANGELOG.md,"
	info "                      leaving an empty [Unreleased] at the top"
	info "  5. release notes    target/release-artifacts/$TAG/RELEASE-NOTES-$TAG.md"
	info "                      (generated from the same [Unreleased] body)"
	info "  6. SHA256SUMS       target/release-artifacts/$TAG/SHA256SUMS"
	info "                        <sha256>  $(artifact_name hermes-rs)"
	info "                        <sha256>  $(artifact_name hermes-rs-session-adapter)"
	info "                      hashes are computed at cut time from the built"
	info "                      binaries; no hash is known or shown before the build"
	info "  7. commit           git add -- CHANGELOG.md && git commit"
	info "                      subject: 'release: $TAG'"
	info "  8. tag              git tag -a $TAG -m 'hermes-rs $TAG'   (local only)"
	info ""
	info "Not in the plan: no push, no GitHub Release, no artifact upload. Pushing"
	info "$TAG is what runs .github/workflows/release.yml (x86_64 + aarch64 builds,"
	info "SHA256SUMS, Release object). See docs/releases.md."
	info ""
	info "CHANGELOG section a cut would write (verbatim):"
	info "---8<---"
	info "## [$VERSION] - $TODAY"
	info ""
	printf '%s\n' "$UNRELEASED_BODY"
	info "---8<---"
	info ""

	rendered="$tmpdir/CHANGELOG.md"
	render_next_changelog "$TODAY" >"$rendered"
	rendered_sha="$(sha256sum "$rendered" | awk '{print $1}')"

	info "Path checks exercised by this dry run (temporary files only)"
	if ! grep -Eq "^## \[${VERSION//./\\.}\] - $TODAY$" "$rendered"; then
		fail "CHANGELOG transform did not produce the dated heading for $VERSION"
	fi
	if ! grep -Eq '^## \[Unreleased\]$' "$rendered"; then
		fail "CHANGELOG transform dropped the [Unreleased] heading"
	fi
	if [ -s "$rendered" ] && [ -n "$(tail -c 1 "$rendered")" ]; then
		fail "CHANGELOG transform did not end with a newline"
	fi
	info "  changelog transform : ok — dated heading for $VERSION present,"
	info "                        [Unreleased] kept, $(grep -c . "$rendered") non-blank lines,"
	info "                        sha256 of the rendered preview: $rendered_sha"
	info "                        (preview discarded; CHANGELOG.md is unchanged)"

	synthetic_note="$tmpdir/sums"
	mkdir -p "$synthetic_note"
	: >"$synthetic_note/$(artifact_name hermes-rs)"
	: >"$synthetic_note/$(artifact_name hermes-rs-session-adapter)"
	(
		cd "$synthetic_note"
		sha256sum "$(artifact_name hermes-rs)" "$(artifact_name hermes-rs-session-adapter)" \
			>"SHA256SUMS"
		sha256sum -c SHA256SUMS >/dev/null
	)
	if [ "$(grep -Ec '^[0-9a-f]{64}  hermes-rs(-session-adapter)?-'"$TAG"'-'"$HOST_TARGET"'$' \
		"$synthetic_note/SHA256SUMS")" -ne 2 ]; then
		fail "SHA256SUMS line format check failed"
	fi
	info "  manifest format     : ok — 2 lines matching '<sha256>  <artifact-name>'"
	info "                        (format check on synthetic zero-byte inputs; these are"
	info "                        not artifact hashes and prove nothing about a build)"
	info "  worktree unchanged  : $(worktree_state) (git status --porcelain)"
	info ""
	info "DRY RUN COMPLETE — nothing was created. rc=0"
}

# ---------------------------------------------------------------------------
# Real cut
# ---------------------------------------------------------------------------

run_gate() {
	info "-- $*"
	if ! run_in_root "$@"; then
		fail "gate failed: $*"
	fi
}

check_remote_tag() {
	local remote_tags
	if ! remote_tags="$(git -C "$ROOT" ls-remote --tags origin "refs/tags/$TAG" 2>/dev/null)"; then
		refuse "could not complete the origin tag check (git ls-remote failed) — refusing rather than cutting a tag that may already exist remotely"
	fi
	if [ -n "$remote_tags" ]; then
		refuse "tag $TAG already exists on origin: $remote_tags"
	fi
}

run_cut() {
	local out_dir bin src

	[ "$WORKTREE" = "clean" ] ||
		refuse "the working tree is dirty — commit or stash every tracked and untracked change first (git status --porcelain is not empty)"
	[ "$BRANCH" = "main" ] ||
		refuse "HEAD is on '${BRANCH:-detached HEAD}', not main — releases are cut from main"
	if local_tag_exists; then
		refuse "tag $TAG already exists locally — delete or re-point it deliberately before cutting"
	fi
	check_remote_tag
	[ -z "$RELEASED_SECTION" ] ||
		refuse "CHANGELOG.md already has a released section for $VERSION ('$RELEASED_SECTION') — bump Cargo.toml or edit the changelog deliberately"
	[ -n "$UNRELEASED_BODY" ] ||
		refuse "the [Unreleased] section of CHANGELOG.md is empty — there is nothing to release"
	[ -n "$READINESS" ] ||
		refuse "a real cut requires --confirm-readiness <row>, naming the readiness sweep row that returned GO — this driver cannot decide readiness for you"

	info "release.sh: cutting $TAG (readiness row: $READINESS)"
	info ""

	info "== gates =="
	run_gate cargo fmt --all -- --check
	run_gate cargo clippy --workspace --all-targets --all-features -- -D warnings
	run_gate cargo test --workspace --all-features

	info ""
	info "== artifacts (host target: $HOST_TARGET) =="
	if ! run_in_root cargo build --release --locked --target "$HOST_TARGET" \
		--bin hermes-rs --bin hermes-rs-session-adapter; then
		fail "artifact build failed — nothing was written to CHANGELOG.md and no tag exists"
	fi

	out_dir="$ROOT/target/release-artifacts/$TAG"
	mkdir -p "$out_dir"
	for bin in "${BINARIES[@]}"; do
		src="$ROOT/target/$HOST_TARGET/release/$bin"
		[ -x "$src" ] || fail "expected build artifact missing: $src"
		cp -- "$src" "$out_dir/$(artifact_name "$bin")"
	done
	(
		cd "$out_dir"
		sha256sum hermes-rs-*
	) >"$out_dir/SHA256SUMS"
	(
		cd "$out_dir"
		sha256sum -c SHA256SUMS >/dev/null
	) || fail "SHA256SUMS does not verify against the built artifacts"
	info "-- $(cd "$out_dir" && pwd)/SHA256SUMS"
	cat "$out_dir/SHA256SUMS"

	{
		printf '# hermes-rs %s\n\n' "$TAG"
		printf 'hermes-rs is an independent Rust project. It is not an official Nous\n'
		printf 'Research release and it is not a drop-in replacement for Hermes Agent.\n'
		printf 'No compatibility or performance claim is made beyond the committed\n'
		printf 'evidence in docs/evidence/.\n\n'
		printf 'Artifacts and checksums are listed in SHA256SUMS. Verify with\n'
		printf '`sha256sum -c SHA256SUMS` before use.\n\n'
		printf '%s\n' "$UNRELEASED_BODY"
	} >"$out_dir/RELEASE-NOTES-$TAG.md"
	info "-- $(cd "$out_dir" && pwd)/RELEASE-NOTES-$TAG.md"

	info ""
	info "== CHANGELOG =="
	TMP_CHANGELOG="$ROOT/.CHANGELOG.md.release-tmp"
	render_next_changelog "$TODAY" >"$TMP_CHANGELOG"
	mv -- "$TMP_CHANGELOG" "$CHANGELOG"
	TMP_CHANGELOG=""
	git -C "$ROOT" add -- CHANGELOG.md
	if git -C "$ROOT" diff --cached --quiet -- CHANGELOG.md; then
		fail "CHANGELOG.md did not change — refusing to commit a no-op release"
	fi
	run_in_root git commit -q -F - <<EOF
release: $TAG

Cut by scripts/release.sh from the [Unreleased] section.
Readiness acknowledgment: --confirm-readiness $READINESS.

Built locally (not published by this script):
  $(artifact_name hermes-rs)
  $(artifact_name hermes-rs-session-adapter)
  SHA256SUMS (target/release-artifacts/$TAG/SHA256SUMS)

No tag was pushed and no release was published by this script. Publishing runs
in .github/workflows/release.yml when the tag is pushed.

hermes-rs is an independent project: no compatibility or performance claim.
EOF

	run_in_root git tag -a "$TAG" -m "hermes-rs $TAG"

	info ""
	info "release.sh: cut $TAG locally."
	info "  commit: $(git -C "$ROOT" rev-parse HEAD)"
	info "  tag   : $TAG -> $(git -C "$ROOT" rev-parse --short "$TAG^{commit}")"
	info "  NOT pushed, NOT published. To publish (separate, deliberate step):"
	info "    git push origin main"
	info "    git push origin $TAG"
}

if [ "$MODE" = "dry-run" ]; then
	print_dry_run
	exit 0
fi

run_cut
