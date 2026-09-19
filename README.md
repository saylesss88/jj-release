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
writes a `CHANGELOG.md` entry, creates a tag, and pushes, all in one step.

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
# Auto-detect environment and generate a release.toml
jj-release init

# Check everything is ready before releasing
jj-release validate

# Full release pipeline
jj-release

# Dry run, see what would happen without changing anything
jj-release --dry-run

# Print the next version that would be released
jj-release next-version

# Preview the next changelog section
jj-release changelog

# Prepend the next section to an existing CHANGELOG.md
jj-release changelog -o CHANGELOG.md

# Regenerate the full changelog from all tags (useful for bootstrapping)
jj-release changelog --full -o CHANGELOG.md

# Open a PR for review before publishing
jj-release pr
```

---

## Getting Started

The fastest way to set up a new repo:

```sh
jj-release init     # detects forge, language, workspace: writes release.toml
jj-release validate # confirms everything is ready
```

`init` detects:

- Forge: from the git remote URL (`github.com` → github, `gitlab.com` → gitlab,
  `codeberg.org` → forgejo)
- Language: from files present (`Cargo.toml` → cargo, `package.json` → npm,
  `go.mod` → go)
- Workspace: reads [workspace.members] from `Cargo.toml` and auto-populates
  member names, paths, and versioning strategy (unified vs independent)

`validate` checks:

- `jj` identity configured
- Forge CLI available (`gh`, `glab`)
- Runs `cargo-semver-checks` to check for breaking API changes & correct release
  version
- Manifest readable, version parsable, not yet published on `crates.io`
- Version tag exists (needed as a baseline)
- Trigger commit present
- `CARGO_REGISTRY_TOKEN` set/`.cargo/credentials.toml` present (if publishing to
  `crates.io`)

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

If a tag doesn't already exist, `jj-release` will create one for you.

### Releasing 1.0.0

When you're ready to release 1.0.0, set `force` in `release.toml`:

```toml
[bump]
force = "major"
```

This overrides the automatic bump calculation regardless of commit types. Remove
it after the release.

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
4. Bumps the version in manifest
5. Creates a `chore: release vX.Y.Z` commit containing both changes
6. Tags the commit with `vX.Y.Z`
7. Advances the bookmark and pushes to origin
8. Runs `cargo publish`
9. Creates a GitHub release (if `forge = "github"`)

The `CARGO_REGISTRY_TOKEN` environment variable is only needed in CI where
there's no credentials file.

---

## PR Workflow

If you want to review before it publishes, use the `pr` subcommand instead:

```sh
jj new -m "Release: please"
jj-release pr
```

This pushes your current bookmark to a `release/vX.Y.Z` branch and opens a PR
with the changelog preview as the PR body. No version bump, no release commit,
no tag. Merge the PR and CI runs `jj-release` to do the actual release.

---

## Configuration

Drop a `release.toml` in your repo root, or run `jj-release init` to generate
one automatically. All fields are optional, defaults work for most GitHub + Rust
projects out of the box:

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
require_tag = true           # require a version tag baseline before releasing

manifest_backend = "cargo"   # manifest format: cargo, npm, go
```

---

## Exit Codes

`jj-release` uses distinct exit codes so CI scripts can distinguish failure
modes:

| Code | Meaning                                      |
| ---- | -------------------------------------------- |
| 0    | Success, or no trigger found (nothing to do) |
| 1    | Unexpected Error                             |
| 2    | Missing or invalid configuration             |
| 3    | Missing required CLI tool (`gh`, `glab`)     |
| 101  | General release failure                      |

This was adapted from `cargo-release`'s error handling.

---

## Forge Support

`jj-release` supports multiple forges for release creation and PR/MR opening:

| Forge            | `forge =`   | CLI required | Release | PR/MR  | Status                |
| ---------------- | ----------- | ------------ | ------- | ------ | --------------------- |
| GitHub (default) | `"github"`  | `gh`         | ✓       | ✓      | ✓ tested              |
| GitLab           | `"gitlab"`  | `glab`       | ✓       | ✓ (MR) | implemented, untested |
| Forgejo/Codeberg | `"forgejo"` | none (REST)  | ✓       | ✓      | implemented, untested |
| None             | `"none"`    | –            | –       | –      | –                     |

For GitLab, set `forge_url` if using a self-hosted instance:

```toml
[release]
forge = "gitlab"
forge_url = "https://gitlab.example.com"
```

For Forgejo/Codeberg, set `forge_url` to your instance and provide a token:

```toml
[release]
forge = "forgejo"
forge_url = "https://codeberg.org"
```

```sh
export FORGEJO_TOKEN=your-token
```

The token needs `repository` scope, create one at
`https://codeberg.org/user/settings/applications`.

---

## Multi-Language Support

> [!NOTE] npm and Go support is implemented but not yet battle-tested in
> production. Feedback welcome if you use `jj-release` with these languages.

`jj-release` supports multiple manifest formats via `manifest_backend`:

| Language   | `manifest_backend =` | Version file   | Publish command |
| ---------- | -------------------- | -------------- | --------------- |
| Rust       | `"cargo"` (default)  | `Cargo.toml`   | `cargo publish` |
| JavaScript | `"npm"`              | `package.json` | `npm publish`   |
| Go         | `"go"`               | tag-only       | –               |

---

## Workspace Support

`jj-release` supports Rust workspaces with two versioning strategies. Run
`jj-release init` to auto-detect which one your workspace uses.

### Unified Versioning

All members share a single version from [workspace.package]. Every member bumps
together on each release:

```toml
[workspace]
enabled = true
versioning = "unified" # or "independent"

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

### Independent Versioning

Each member has its own version and only bumps when commits touched its path.
Members get their own tag prefix:

```toml
[workspace]
enabled = true
versioning = "independent"

[[workspace.members]]
name = "mylib"
path = "lib"
publish = true
tag_prefix = "mylib-v"   # creates tags like mylib-v0.5.0

[[workspace.members]]
name = "mycli"
path = "cli"
publish = true
tag_prefix = "v"
depends_on = ["mylib"]
```

> [!NOTE] Independent versioning requires `cargo-edit` for version bumping:
>
> ```sh
> cargo install cargo-edit
> ```

Members are published in dependency order: `mylib` before `mycli`. So
`crates.io` has time to index the library before the CLI tries to depend on it.

```sh
jj-release --dry-run
 Current version: 0.7.0
[dry-run] Independent workspace release:
[dry-run] Would publish in order:
  - mylib (lib) 0.4.0 (no changes, skipping)
  - mycli (cli) 0.7.0 → 0.8.0 (tag: mycli-v0.8.0)
```

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

The highest bump across all commits since the last tag wins. Scoped commits are
preserved in the changelog. `feat(cli): add init subcommand` renders as
`**(cli)** add init subcommand` under `### Added`

> [!NOTE] `chore:`, `docs:`, `style:`, `test:`, `ci:`, and `build:` commits do
> not trigger a release. If your only changes since the last tag are in these
> categories, jj-release will exit with "No releasable commits". Either add a
> `feat:` or `fix:` commit, or force a bump in `release.toml`:
>
> ```toml
> [bump]
> force = "patch"
> ```
>
> Remove `force` after the release so future versions are computed
> automatically.

---

## Changelog format

`jj-release` follows the [Keep a Changelog](https://keepachangelog.com) format.
Each release prepends a new section to `CHANGELOG.md`:

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-09-08

### Added

- **(cli)** add init subcommand
- add wallpaper cycling support

### Fixed

- prevent daemon crash on empty directory
```

---

## GitHub Actions

> [!NOTE] This action takes a while to finish since it compiles `jj` and
> `jj-release` from source. Use `jj-release` locally if you're in a hurry.

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
