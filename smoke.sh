#!/usr/bin/env bash
# Smoke-test jj-release against scratch copies of local repos.
#
# Safe by construction: every repo is copied into a temp dir first, and the
# script only runs read-only commands (next-version, validate, stdout
# changelogs) and dry runs (-n). It never runs a real release, so nothing is
# committed, tagged, pushed or published outside the scratch copies.
#
# Usage: edit the four variables below, then
#   bash jj-release-smoke.sh 2>&1 | tee smoke.log
set -uo pipefail

# --- Configure -------------------------------------------------------------
SINGLE="$HOME/dev/mdbook-frontmatter-strip"  # a single-crate repo
RIPGREP="$HOME/dev/ripgrep"                  # a ripgrep-style workspace
TRIGGER="Release: please"                                   # your release.trigger message
PREFIX="v"                                   # SINGLE's tag prefix
# ---------------------------------------------------------------------------

[[ -n "$TRIGGER" ]] || { echo "set TRIGGER to your release.trigger message"; exit 1; }

SCRATCH=$(mktemp -d -t jj-release-smoke.XXXX)
trap 'echo; echo "scratch copies kept in $SCRATCH"' EXIT

section() { printf '\n==================== %s ====================\n' "$*"; }
expect()  { printf '  expect: %s\n' "$*"; }
run()     { printf '$ %s\n' "$*"; "$@" 2>&1 | sed 's/^/    /'; }

# Copy a repo without its build output.
copy_repo() {
    local dest="$SCRATCH/$(basename "$1")"
    if command -v rsync >/dev/null; then
        rsync -a --exclude target/ "$1/" "$dest/"
    else
        cp -a "$1" "$dest" && rm -rf "$dest/target"
    fi
    printf '%s\n' "$dest"
}

# Tag a revision, with `jj tag set` if this jj has it, else via colocated git.
tag() {
    if jj tag set --help >/dev/null 2>&1; then
        jj tag set "$1" -r "$2" >/dev/null
    else
        git tag "$1" "$(jj log -r "$2" --no-graph -T commit_id)" && jj git import >/dev/null
    fi
}

# Last line of `next-version`, if it looks like a version.
next_version() {
    jj-release next-version "$@" 2>/dev/null | tail -n1 | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?' | tail -n1
}

run jj-release --version

# ---------------------------------------------------------------------------
section "Single crate: pre-release flow ($SINGLE)"
cd "$(copy_repo "$SINGLE")" || exit 1

jj new -m "feat: smoke-test feature" >/dev/null
jj new -m "$TRIGGER" >/dev/null

run jj-release validate
run jj-release next-version
expect "the next stable version (call it X)"

RC1=$(next_version --pre rc)
run jj-release next-version --pre rc
expect "X-rc.1"
[[ -n "$RC1" ]] || { echo "  !! couldn't read a version from next-version; stopping this section"; RC1=""; }

if [[ -n "$RC1" ]]; then
    tag "$PREFIX$RC1" @
    echo "  (tagged @ as $PREFIX$RC1 to simulate the rc.1 release)"

    jj new -m "fix: smoke-test fix after rc.1" >/dev/null
    jj new -m "$TRIGGER" >/dev/null

    run jj-release next-version --pre rc
    expect "${RC1%.1}.2   (same channel increments)"

    run jj-release next-version --pre beta
    expect "an error: beta would sort below the existing rc"

    run jj-release next-version --pre rc.1
    expect "an error: invalid pre-release channel"

    run jj-release next-version
    expect "${RC1%%-*}   (no --pre promotes to stable)"

    run jj-release -n --pre rc
    expect "dry run for ${RC1%.1}.2; distro packages skipped (pre-release)"

    run jj-release -n
    expect "dry run for ${RC1%%-*}; distro packages listed if configured"

    run jj-release changelog
    expect "## [Unreleased] with the fix (manifest isn't bumped in this copy)"

    run jj-release changelog --full
    expect "## [$RC1] - <today> with the feature, [Unreleased] above it with the fix"
fi

# ---------------------------------------------------------------------------
section "Workspace: ripgrep layout ($RIPGREP)"
cd "$(copy_repo "$RIPGREP")" || exit 1

# One change in a leaf crate that other members depend on.
echo "// smoke test" >> crates/globset/src/lib.rs
jj commit -m "fix(globset): smoke-test fix" >/dev/null
jj new -m "$TRIGGER" >/dev/null

run jj-release validate
run jj-release next-version

run jj-release -n
expect "globset bumped; dependents (ignore, grep, ripgrep) propagated if their requirements no longer match"

run jj-release -n --pre rc
expect "globset X-rc.1; every dependent also gets a pre-release (requirements don't match rc versions)"

for layout in root per-member both; do
    run jj-release changelog --layout "$layout"
done
expect "root: one section (unified) or named per-member sections (independent)"
expect "per-member: only crates/globset/CHANGELOG.md has an entry"
expect "both: root file + member files, no duplicate entry for a member at ."

run jj-release changelog --output /dev/null --layout per-member
expect "an error: --output writes a single file"

run jj-release changelog --full --layout root
expect "full history, newest first; independent workspaces get named, date-ordered headings"
