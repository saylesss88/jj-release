# jj-release

Automated semantic releases for [Jujutsu](https://github.com/jj-vcs/jj)
repositories.

Unlike tools built around mutable Git branches (like `release-plz` or
`cargo-release`), `jj-release` is designed natively for `jj`. It uses `jj`'s
revset language to walk commit history, operates on movable bookmarks, and uses
a commit-message trigger that fits naturally into the `jj` workflow.

---

## How it works

Add a trigger commit when you're ready to ship:

```sh
jj new -m "Release: please"
jj git push --bookmark main
```

CI detects the trigger, walks commits back to the last version tag, and
classifies them as patch/minor/major using
[Conventional Commits](https://www.conventionalcommits.org). API-breaking
changes are detected via
[cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks) and can
automatically upgrade the bump to major. The version baseline comes from
crates.io rather than the local manifest, ensuring the correct bump even when
versions have drifted. `jj-release`

A trigger is recognized when any commit reachable from `release.bookmark` has a
description containing `release.trigger` (case-sensitive substring match) since
the last version tag. The trigger commit itself is not included in changelog
generation or bump calculation unless its message also matches a Conventional
Commit type.

Accidentally triggering a release with `docs: Release: please improve wording`
is possible, choose a trigger string unlikely to appear in normal commit
messages.

### What a Release Changes

Unless `--dry-run` is used, `jj-release` may:

- Modify version manifests and `CHANGELOG.md`
- Create a release commit and version tag
- Move and push the configured bookmark
- Publish packages to the configured registry
- Create a GitHub, GitLab, or Forgejo release

Run `jj-release validate` and `jj-release --dry-run` before enabling it in CI.

---

## Quick Start

```sh
cargo install jj-release
jj-release init
jj-release validate

# After adding releasable commits:
jj new -m "Release: please"
jj git push --bookmark main
```

On GitHub Actions, add the workflow shown below so pushes containing the trigger
run the release pipeline.

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
- `cargo-semver-checks` detects breaking API changes & warns if bump should be
  major version
- Runs `cargo publish --dry-run`
- Manifest readable, version parseable, and in sync with crates.io
- Version tag exists (needed as a baseline)
- Trigger commit present
- `CARGO_REGISTRY_TOKEN` set/`.cargo/credentials.toml` present (if publishing to
  crates.io)

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

If no version tag exists, `jj-release` automatically creates one from your
current `Cargo.toml` version as a baseline before releasing. You don't need to
create it manually.

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
to crates.io (`cargo login`), you can run it directly (Recommended over CI):

```sh
jj new -m "Release: please"
jj-release
```

Running `jj-release` locally does the full pipeline:

1. Scans for the trigger commit since the last version tag
2. Walks commits and computes the semver bump from conventional commit types
3. Runs pre-flight checks (including `cargo-publish --dry-run`) to ensure the
   release won't fail midway
4. Writes a new section to `CHANGELOG.md`
5. Bumps the version in manifest
6. Creates a `chore: release vX.Y.Z` commit and tags it `vX.Y.Z` locally
7. Runs the actual `cargo publish`
8. Advances the bookmark and pushes to origin
9. Creates a GitHub release (if `create_release = true`)

The `CARGO_REGISTRY_TOKEN` environment variable is only needed in CI where
there's no credentials file.

Fail-Safe Design: `jj-release` delays irreversible remote actions (like
`git push`) until the very end. If a step like `cargo publish` fails, your
remote git repository remains completely untouched. Because you are using
Jujutsu, you can simply `jj abandon` the failed local release commit, fix the
underlying issue, and run `jj-release` again.

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

<!-- prettier-ignore -->
> [!NOTE]
> Unlike release-plz's continuously maintained release PR, `jj-release pr` is a
> one-shot preview. No version bump or release commit in the PR itself. If you
> want a persistent release PR that stays up to date, run `jj-release pr` in CI
> on every push to main.

---

## Configuration

Drop a `release.toml` in your repo root, or run `jj-release init` to generate
one automatically. All fields are optional, defaults work for most GitHub + Rust
projects out of the box:

```toml
[release]
trigger = "Release: please"          # commit message substring that kicks off a release
tag_prefix = "v"                     # prefix for version tags, e.g. v1.2.3
bookmark = "main"                    # bookmark to advance after release
forge = "github"                     # forge for releases and PRs: github, gitlab, forgejo, or none
create_release = false               # create a release on the forge after tagging
forge_url = ""                       # base URL for self-hosted forges

[bump]
# force = "minor"                    # override commit analysis: "major", "minor", or "patch"

[publish]
cargo = true                         # run `cargo publish` after tagging
cargo_flags = []                     # extra flags forwarded to `cargo publish`
semver_checks = true                 # run cargo-semver-checks before publishing
semver_checks_upgrade_major = false  # auto-upgrade to major (default: only post-1.0)

[changelog]
enabled = true                       # write a CHANGELOG.md entry on each release
file = "CHANGELOG.md"                # path to the changelog file
require_tag = true                   # require a version tag baseline; auto-creates one if missing

manifest_backend = "cargo"           # manifest format: cargo, npm, go
```

By default, Cargo projects publish to crates.io after the tag is pushed. Set
`[publish] cargo = false` to use jj-release only for versioning, changelogs,
tags, and forge releases.

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

| Forge   | Authentication                                                      |
| ------- | ------------------------------------------------------------------- |
| GitHub  | `gh auth login` locally or `GITHUB_TOKEN` in GitHub Actions         |
| GitLab  | `glab auth login` locally or the token/env mechanism used by `glab` |
| Forgejo | `FORGEJO_TOKEN` and `forge_url`                                     |

---

## Multi-Language Support

<!-- prettier-ignore -->
> [!NOTE]
> npm and Golang support is implemented but not yet battle-tested in production.
> Feedback welcome if you use `jj-release` with these languages.

`jj-release` supports multiple manifest formats via `manifest_backend`:

| Language   | `manifest_backend =` | Version file   | Publish command |
| ---------- | -------------------- | -------------- | --------------- |
| Rust       | `"cargo"` (default)  | `Cargo.toml`   | `cargo publish` |
| JavaScript | `"npm"`              | `package.json` | `npm publish`   |
| Golang     | `"go"`               | tag-only       | –               |

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

<!-- prettier-ignore -->
> [!NOTE]
> Independent versioning requires `cargo-edit` for version bumping:
>
> ```sh
> cargo install cargo-edit
> ```

Members are published in dependency order. `mylib` before `mycli`. So crates.io
has time to index the library before the CLI tries to depend on it.

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
| `feat:`                              | minor      |
| `fix:`, `perf:`, `refactor:`         | patch      |
| `feat!:` or `BREAKING CHANGE` footer | major      |
| `chore:`, `docs:`, `test:`, etc.     | No release |

The highest bump across all commits since the last tag wins. Scoped commits are
preserved in the changelog. `feat(cli): add init subcommand` renders as
`**(cli)** add init subcommand` under `### Added`

<!-- prettier-ignore -->
> [!NOTE]
> `chore:`, `docs:`, `style:`, `test:`, `ci:`, and `build:` commits do not
> trigger a release. If your only changes since the last tag are in these
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

<!-- prettier-ignore -->
> [!NOTE]
> This action takes a while to finish since it compiles `jj` and `jj-release`
> from source. Use `jj-release` locally if you're in a hurry.

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
  syntax differs)
- A colocated jj/git repository (`jj git init --colocate`)

---

## Credits

Parts of the codebase are adapted from these great projects:

- Version baseline from crates.io rather than manifest. Check for API breaking
  changes with `cargo-semver-checks`:
  [release-plz](https://github.com/release-plz/release-plz)

- Error handling approach:
  [cargo-release](https://github.com/crate-ci/cargo-release)

## License

- [Apache-2.0](https://github.com/saylesss88/jj-release/blob/main/LICENSE)
