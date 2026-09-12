# jj-release

Automated semantic releases for [Jujutsu](https://github.com/jj-vcs/jj)
repositories.

## Why

[semantic-release](https://github.com/semantic-release/semantic-release),
[release-please](https://github.com/googleapis/release-please), and
[cargo-release](https://github.com/crate-ci/cargo-release) are all great tools,
but they assume Git's branch model. They expect a mutable `main` branch,
branch-based triggers, and tags anchored to branch tips. Jujutsu's branchless,
immutable-commit workflow breaks all of these assumptions in ways that are
annoying to work around.

`jj-release` is built specifically for `jj`: it uses `jj`'s revset language to
walk commit history, works with bookmarks instead of branches, and uses a simple
commit-message trigger that fits naturally into the `jj` workflow.

Inspired by semantic-release, release-please, and cargo-release.

---

## How it works

Add a trigger commit when you're ready to ship:

```sh
jj new -m "Release: please"
jj git push --bookmark main
```

CI detects the trigger, walks commits back to the last version tag, classifies
them as patch/minor/major using
[Conventional Commits](https://www.conventionalcommits.org), bumps `Cargo.toml`,
writes a `CHANGELOG.md` entry, creates a tag, and pushes — all in one step.

---

## Installation

```sh
cargo install jj-release
```

Requires `jj` on your PATH. For GitHub releases, `gh` must also be available in
CI.

---

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

# Open a PR for review before publishing
jj-release pr
```

---

## First release

For a first release, set your `Cargo.toml` version to `0.0.0` and let
`jj-release` compute the initial version from your commit history. If you want
to guarantee `0.1.0`, add a force override in `release.toml`:

```toml
[bump]
force = "minor"
```

Remove `force` after the first release so subsequent versions are computed
automatically from conventional commits.

---

## Local usage

`jj-release` doesn't require CI. If you have `jj` on your PATH and are logged in
to crates.io (`cargo login`), you can run it directly:

```sh
jj new -m "Release: please"
jj-release
```

Running `jj-release` locally does the full pipeline:

1. Scans for the trigger commit since the last version tag
2. Walks commits and computes the semver bump from conventional commit types
3. Writes a new section to `CHANGELOG.md`
4. Bumps the version in `Cargo.toml`
5. Creates a `chore: release vX.Y.Z` commit containing both changes
6. Tags the commit with `vX.Y.Z`
7. Advances the bookmark and pushes to origin
8. Runs `cargo publish`
9. Creates a GitHub release (if `github_release = true`)

The `CARGO_REGISTRY_TOKEN` environment variable is only needed in CI where
there's no credentials file.

---

## PR Workflow

If you want to review the release commit before it publishes, use the pr
subcommand instead:

```sh
jj new -m "Release: please"
jj-release pr
```

This does everything up to publishing: bumps the version, writes the changelog,
creates the release commit. Then pushes to a `release/vX.Y.Z` bookmark and opens
a PR via `gh pr create` or `glab mr create`. Merge the PR and CI runs
`jj-release` to publish.

---

## Configuration

Drop a `release.toml` in your repo root for any overrides you need. All fields
are optional, defaults are shown below:

```toml
[release]
trigger = "Release: please"  # commit message substring that kicks off a release
tag_prefix = "v"             # prefix for version tags, e.g. v1.2.3
bookmark = "main"            # bookmark to advance after release
forge = "github"             # forge for releases and PRs: github, gitlab, none
create_release = false       # create a release on the forge after tagging
forge_url = ""               # base URL for self-hosted forges

[bump]
# force = "minor"            # override commit analysis: "major", "minor", or "patch"

[publish]
cargo = true                 # run `cargo publish` after tagging
cargo_flags = []             # extra flags forwarded to `cargo publish`

[changelog]
enabled = true               # write a CHANGELOG.md entry on each release
file = "CHANGELOG.md"        # path to the changelog file

manifest_backend = "cargo"   # manifest format: cargo, npm, go
```

---

## Forge Support

`jj-release` supports multiple forges for release creation and PR/MR opening:

| Forge            | `forge =`   | CLI required | Release        | PR/MR        |
| ---------------- | ----------- | ------------ | -------------- | ------------ |
| GitHub (default) | `"github"`  | `gh`         | ✓              | ✓            |
| GitLab           | `"gitlab"`  | `glab`       | ✓              | ✓ (MR)       |
| Forgejo/Codeberg | `"forgejo"` | none (REST)  | \ comming soon | comming soon |
| None             | `"none"`    | --           | --             | --           |

For GitLab, set `forge_url` if using a self-hosted instance:

```toml
[release]
forge = "gitlab"
forge_url = "https://gitlab.example.com"
```

---

## Multi-Language Support

`jj-release` supports multiple manifest formats via `manifest_backend`:

| Language   | `manifest_backend =` | Version file   | Publish command |
| ---------- | -------------------- | -------------- | --------------- |
| Rust       | `"cargo"` (default)  | `Cargo.toml`   | `cargo publish` |
| JavaScript | `"npm"`              | `package.json` | `npm publish`   |
| Go         | `"go"`               | tag-only       | --              |

---

## Workspace Support

For Rust workspaces with multiple crates, `jj-release` supports unified
versioning where all members share a single version from [workspace.package]:

```toml
[workspace]
enabled = true
versioning = "unified"

[[workspace.members]]
name = "mylib"
path = "lib"
publish = true

[[workspace.members]]
name = "mycli"
path = "cli"
publish = true
depends_on = ["mylib"]   # publish lib before cli
```

Members are published in dependency order: `mylib` before `mycli`. So
`crates.io` has time to index the library before the CLI tries to depend on it.

---

## Conventional Commits

`jj-release` follows the
[Conventional Commits](https://www.conventionalcommits.org) spec to determine
the version bump:

| Commit type                          | Bump       |
| ------------------------------------ | ---------- |
| `feat:`                              | Minor      |
| `fix:`, `perf:`, `refactor:`         | Patch      |
| `feat!:` or `BREAKING CHANGE` footer | Major      |
| `chore:`, `docs:`, `test:`, etc.     | No release |

The highest bump across all commits since the last tag wins.

---

## Changelog format

`jj-release` follows the [Keep a Changelog](https://keepachangelog.com) format.
Each release prepends a new section to `CHANGELOG.md`:

```markdown
# Changelog

## [0.2.0] - 2026-09-08

### Added

- add wallpaper cycling support
- add IPC command interface

### Fixed

- prevent daemon crash on empty directory
```

---

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

---

## Requirements

- Rust 1.80+
- jj 0.43+ (tested on 0.43.0; earlier versions may work but tag and push flag
  syntax differs: see [jj compatibility](#jj-compatibility) below)
- A colocated jj/git repository (`jj git init --colocate`)

---

## License

- [Apache-2.0](https://github.com/saylesss88/jj-release/blob/main/LICENSE)
