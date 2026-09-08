# jj-release

Automated semantic releases for [Jujutsu](https://github.com/jj-vcs/jj) repositories.

## Why

[semantic-release](https://github.com/semantic-release/semantic-release), [release-please](https://github.com/googleapis/release-please), and [cargo-release](https://github.com/crate-ci/cargo-release) are all great tools, but they assume Git's branch model. They expect a mutable `main` branch, branch-based triggers, and tags anchored to branch tips. Jujutsu's branchless, immutable-commit workflow breaks all of these assumptions in ways that are annoying to work around.

Trying to use semantic-release with jj means fighting the tool constantly: the colocated git repo gets out of sync, branch detection fails, and the trigger model doesn't map cleanly onto how jj users actually work. jj-release is built from scratch for jj: it speaks revsets, works with bookmarks instead of branches, and uses a simple commit-message trigger that fits naturally into the jj workflow.

Inspired by semantic-release, release-please, and cargo-release.

## How it works

Add a trigger commit when you're ready to ship:

```sh
jj new -m "Release: please"
jj git push --bookmark main
```

CI detects the trigger, walks commits back to the last version tag, classifies them as patch/minor/major using [Conventional Commits](https://www.conventionalcommits.org), bumps `Cargo.toml`, writes a `CHANGELOG.md` entry, creates a tag, and pushes — all in one step.

## Installation

```sh
cargo install jj-release
```

Requires `jj` on your PATH. For GitHub releases, `gh` must also be available in CI.

## Usage

```sh
# Full release pipeline
jj-release

# Dry run, see what would happen without changing anything
jj-release --dry-run

# Print the next version that would be released
jj-release next-version

# Generate and print the changelog section without releasing
jj-release changelog
```

## Configuration

Drop a `release.toml` in your repo root. All fields are optional, defaults are shown below:

```toml
[release]
trigger = "Release: please"  # commit message substring that kicks off a release
tag_prefix = "v"             # prefix for version tags, e.g. v1.2.3
bookmark = "main"            # bookmark to advance after release
github_release = false       # create a GitHub release via `gh release create`

[bump]
# force = "minor"            # override commit analysis: "major", "minor", or "patch"

[publish]
cargo = true                 # run `cargo publish` after tagging
cargo_flags = []             # extra flags forwarded to `cargo publish`

[changelog]
enabled = true               # write a CHANGELOG.md entry on each release
file = "CHANGELOG.md"        # path to the changelog file
```

## Conventional Commits

jj-release follows the [Conventional Commits](https://www.conventionalcommits.org) spec to determine the version bump:

| Commit type | Bump |
|---|---|
| `feat:` | Minor |
| `fix:`, `perf:`, `refactor:` | Patch |
| `feat!:` or `BREAKING CHANGE` footer | Major |
| `chore:`, `docs:`, `test:`, etc. | No release |

The highest bump across all commits since the last tag wins.

## Changelog format

jj-release follows the [Keep a Changelog](https://keepachangelog.com) format. Each release prepends a new section to `CHANGELOG.md`:

```markdown
# Changelog

## [0.2.0] - 2026-09-08

### Added
- add wallpaper cycling support
- add IPC command interface

### Fixed
- prevent daemon crash on empty directory
```

## GitHub Actions

Add this workflow to your consumer repo at `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    branches:
      - main

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
          token: ${{ secrets.GITHUB_TOKEN }}

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Install jj
        run: cargo install jj-cli --locked

      - name: Install jj-release
        run: cargo install jj-release --locked

      - name: Attach jj to colocated git repo
        run: jj git init --colocate

      - name: Run release
        run: jj-release
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

## Requirements

- Rust 1.80+
- jj 0.21+
- A colocated jj/git repository (the default for most jj users on GitHub)

## License

Apache-2.0
