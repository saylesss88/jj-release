# Configuration

Drop a `release.toml` in your repo root, or run `jj-release init` to generate
one automatically. All fields are optional, defaults work for most GitHub + Rust
projects out of the box:

```toml
manifest_backend = "cargo"           # manifest format: cargo, npm, go

[release]
trigger = "Release: please"          # commit message substring that kicks off a release
tag_prefix = "v"                     # prefix for version tags, e.g. v1.2.3
bookmark = "main"                    # bookmark to advance after release
forge = "github"                     # github, gitlab, forgejo, gitea, or none
create_release = false               # create a release on the forge after tagging
forge_url = ""                       # base URL for self-hosted forges

[bump]
# force = "minor"                    # override commit analysis: "major", "minor", or "patch"
# pre = "rc"                         # release pre-releases on this channel: 1.0.0-rc.1, -rc.2, …
features_bump_patch_before_1_0 = false  # before 1.0, release features as patches (0.4.1 → 0.4.2)

[publish]
cargo = false                        # set to true to publish to crates.io
cargo_flags = []                     # extra flags forwarded to `cargo publish`
semver_checks = true                 # run cargo-semver-checks before publishing
semver_checks_upgrade_major = false  # also upgrade pre-1.0 crates to a breaking bump

[changelog]
enabled = true                       # write a CHANGELOG.md entry on each release
file = "CHANGELOG.md"                # path to the changelog file
strict = true                        # drop unmapped/unformatted commits (false = "Other Changes")
link_headers = false                 # link each version heading to a forge compare view
layout = "root"                      # "root", "per-member", or "both" (workspaces)

# [aur]
# Required: The exact name of your package on the AUR
# package = "my-cli-tool"

# Optional: Override the default Git URL.
# Defaults to ssh://aur@aur.archlinux.org/{package}.git
# repo = "ssh://aur@aur.archlinux.org/custom-repo-name.git"

# [copr]
# Required: your COPR project as owner/project
# project = "your-username/my-cli-tool"

# Required: path to the spec, relative to the repo root
# spec = "packaging/rust-my-cli-tool.spec"

# Optional: wait for the COPR build to finish (default: false)
# wait = true
```

`manifest_backend` is a top-level key, so it must come before the first
`[table]` header. TOML puts any key written after a header inside that table.

The `forgejo` and `gitea` forges are behind Cargo features of the same name.

## Publishing

By default, `jj-release` only handles versioning, changelogs, tags, and forge
releases. To automatically publish to crates.io, explicitly enable it in your
configuration:

```toml
[publish]
cargo = true
```

Publishing can also be skipped per-run with `--no-publish` without editing
`release.toml`, useful for testing the release pipeline or in scripts that
handle publishing separately. `--publish-cargo` (`-p`) does the opposite.

Pre-releases are never sent to the AUR or COPR. Distro packages are only
updated for stable versions.

## Per-run overrides

These flags override `release.toml` for a single run, so there's nothing to
change and change back:

| Flag | Overrides | Example |
|---|---|---|
| `--bump <LEVEL>` | `bump.force` | `--bump major` releases 1.0.0 from 0.x |
| `--pre <CHANNEL>` | `bump.pre` | `--pre rc` releases 1.0.0-rc.1, then -rc.2, … |
| `--layout <LAYOUT>` | `changelog.layout` | `--layout both` |
| `--no-publish` / `-p` | `publish.cargo` | `--no-publish` |

## Versioning before 1.0

Before 1.0.0, Cargo treats the minor version as the breaking position, and
`jj-release` follows it: a breaking change (`feat!:`, `BREAKING CHANGE:`)
bumps the minor version (0.10.0 → 0.11.0), never the major. Moving to 1.0.0
is always your call:

```sh
jj-release --bump major
```

Features bump the minor version by default (0.4.21 → 0.5.0). For a library
other crates depend on, release them as patches instead, so dependents
requiring `"0.4"` pick them up without a requirement change:

```toml
[bump]
features_bump_patch_before_1_0 = true
```

## Pre-releases

`--pre <channel>` (or `bump.pre`) releases the next pre-release instead of a
stable version. A run without it promotes the latest pre-release to stable:

```sh
jj-release --bump major --pre rc   # 0.11.0 → 1.0.0-rc.1
jj-release --pre rc                # 1.0.0-rc.2
jj-release --pre beta              # refused: beta sorts below rc
jj-release                         # 1.0.0
```

Moving up a channel (`alpha` → `beta` → `rc`) restarts the number at `.1`.

## Workspaces

`jj-release init` detects a Cargo workspace and writes this section for you:

```toml
[workspace]
enabled = true
versioning = "independent"           # "unified": one version for every member

[[workspace.members]]
name = "globset"
path = "crates/globset"
tag_prefix = "globset-"              # tags look like globset-0.4.22

[[workspace.members]]
name = "ignore"
path = "crates/ignore"
tag_prefix = "ignore-"
depends_on = ["globset"]             # members it depends on: sets publish order
```

With independent versioning, each member is released from its own tags and
only when its files changed. When a member's new version no longer satisfies
another member's requirement on it, that dependent is released too.

## Version bumps in other files

`jj-release` bumps `version = "..."` in a `flake.nix` at the repo root
automatically. For anything else, such as install snippets in a README, add a
replacement:

```toml
[[replacements]]
file = "README.md"
search = 'my-crate = "[0-9.]+"'
replace = 'my-crate = "{{version}}"'
exactly_one = true                   # fail unless the pattern matches exactly once
```

`replace` can use `{{version}}`, `{{crate_name}}`, and `{{date}}`.

## Changelog Configuration

You can customize how `jj-release` parses your commits and maps them to the Keep
a Changelog format. By default, these commit types are included:

| Commit type | Changelog section |
|---|---|
| `feat` | Added |
| `refactor` | Changed |
| `perf` | Performance |
| `deprecate`, `deprecated` | Deprecated |
| `remove`, `removed` | Removed |
| `fix` | Fixed |
| `bug` | Bug |
| `security` | Security |
| `docs` | Documentation |

Anything else (`chore`, `ci`, `test`, unknown types, and messages that aren't
Conventional Commits) is dropped, or collected under "Other Changes" with
`strict = false`. Breaking changes are marked `**BREAKING**` in their section.

Add these options to your `release.toml` to customize the behavior:

```toml
[changelog]
enabled = true
file = "CHANGELOG.md"
# Add emojis to the changelog headers
emoji_headers = true

# If true (default), commits that do not strictly match Conventional Commits
# or your prefix_mapping are silently dropped from the changelog.
# If false, unformatted commits are collected in an "Other Changes" section.
strict = false

# Commits to leave out of the changelog, matched as a whole word at the start
# of the message: "fix" excludes `fix: …`, `fix(cli): …` and `fix!: …`, but not
# "fixed a typo". Use "chore(deps)" to exclude only that scope.
exclude_prefixes = ["chore", "ci", "test", "WIP:"]

# Override specific default emojis (keys must match the exact Header name)
[changelog.emoji_mapping]
Added = "🚀"
Fixed = "✅️"
Bug = "🔥"
Performance = "⚡️"
Testing = "🧪"

# Map conventional commit types to custom Keep a Changelog headers.
# A mapped type is included even with strict = true. Custom headers appear
# after the standard ones, in alphabetical order.
[changelog.prefix_mapping]
revert = "Reverted"
test = "Testing"
```
