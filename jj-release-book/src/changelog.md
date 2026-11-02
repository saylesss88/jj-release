# Changelogs

`jj-release` follows the [Keep a Changelog](https://keepachangelog.com) format.
Each release prepends a new section to `CHANGELOG.md`:

```bash
# Preview the next changelog section
jj-release changelog

# Prepend the next section(s) to the configured changelog file(s)
jj-release changelog -w

# Prepend the next section to a specific file (root layout only)
jj-release changelog -o NOTES.md

# Regenerate the full changelog from all tags (useful for bootstrapping)
jj-release changelog --full -w

# Override to a specific layout from the command-line (root, per-member, or both)
jj-release changelog --layout both
```

`-w` (`--write`) writes to `[changelog].file` (default `CHANGELOG.md`): at the
repository root, in each workspace member's directory, or both, depending on
[`layout`](#workspace-layouts). `-o` writes to one explicit path, so it only
works with the root layout; `-o` and `-w` can't be combined.

<!--prettier-ignore-->
> [!NOTE]
> Without `--full`, `-w` **prepends** the new section. With `--full`, it
> **overwrites** the file with the regenerated history.

If the manifest version hasn't been bumped past the latest tag, the preview
renders the pending changes as `## [Unreleased]`. Once released, the section
gets its version and the release date.

### Basic Changelog Example

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

## [0.2.0] - 2026-09-08

### Added

- **(cli)** add init subcommand
- add wallpaper cycling support

### Fixed

- prevent daemon crash on empty directory
```

Scopes (`feat(cli): ...`) are shown in bold, and breaking changes (`feat!: ...`)
are marked **BREAKING**.

### Emoji headers and compare links

With `emoji_headers = true` and `link_headers = true`:

```markdown
## [Unreleased](https://github.com/saylesss88/jj-release/compare/v0.2.0...HEAD)

## [0.2.0](https://github.com/saylesss88/jj-release/compare/v0.1.0...v0.2.0) - 2026-09-08

### ✨ Added

- **(cli)** add init subcommand
- add wallpaper cycling support

### 🛠️ Fixed

- prevent daemon crash on empty directory
```

Compare links work for GitHub, GitLab, Codeberg/Forgejo, and Gitea. Self-hosted
instances need `[release].forge_url` set.

## Changelog Configuration

By default, these commit types are included:

| Commit type               | Section       |
| ------------------------- | ------------- |
| `feat`                    | Added         |
| `refactor`                | Changed       |
| `perf`                    | Performance   |
| `deprecate`, `deprecated` | Deprecated    |
| `fix`                     | Fixed         |
| `bug`                     | Bug           |
| `docs`                    | Documentation |

`style`, `chore`, `build`, `ci` and `test` are dropped in strict mode (the
default). Any other type can be mapped to a section with `prefix_mapping`.

```toml
[changelog]
enabled = true
file = "CHANGELOG.md"
# Add emojis to the section headers
emoji_headers = true
# Link version headings to the forge's compare page
link_headers = true
# "root" (default), "per-member", or "both". See Workspace layouts below.
layout = "per-member"
# If true (default), commits that don't parse as Conventional Commits, or
# whose type isn't mapped, are dropped. If false, they're collected in an
# "Other Changes" section.
strict = false

# Commit types or raw message prefixes to always exclude.
exclude_prefixes = ["chore", "ci", "test", "WIP:"]

# Override default emojis (keys are section names).
[changelog.emoji_mapping]
Added = "🚀"
Fixed = "✅️"
Bug = "🔥"
Performance = "⚡️"
Testing = "🧪"

# Map commit types to sections. Built-in types are mapped automatically.
[changelog.prefix_mapping]
revert = "Reverted"
security = "Security"
test = "Testing"
```

Custom sections appear after the standard ones, sorted alphabetically.

## Workspace layouts

In a workspace, `layout` controls where changelogs are written:

| `layout`       | Writes                                              |
| -------------- | --------------------------------------------------- |
| `"root"`       | One `CHANGELOG.md` at the repository root (default) |
| `"per-member"` | One `CHANGELOG.md` in each member's directory       |
| `"both"`       | Each member's `CHANGELOG.md` plus one at the root   |

### Which commits go where

A member's changelog contains the commits that **changed files in that member's
directory**. The commit message's scope doesn't matter: `feat(cli):` lands in
`lib/CHANGELOG.md` if it only touched files under `lib/`.

- A commit touching two members appears in both changelogs.
- A commit touching only files outside every member (CI, README, the workspace
  `Cargo.toml`) appears in no member changelog. Use `"both"` with unified
  versioning to keep a record of those in the root changelog.
- A member at the repository root (a root package, like `ripgrep` in its own
  workspace) doesn't include the other members' directories, even though they
  sit inside it.

To check what a member's next changelog will contain, run the same query
`jj-release` uses:

```bash
jj log -r 'v0.10.0..@ & files(root:"lib")'
```

### The root changelog with `"both"`

- **Unified versioning:** the root changelog gets a normal section listing every
  commit in the release, including changes outside any member.
- **Independent versioning:** the root changelog gets one named section per
  released member:

  ```markdown
  ## globset [0.5.0] - 2026-11-02

  ### Added

  - **(glob)** support brace expansion

  ## ignore [0.4.35] - 2026-11-02

  ### Dependencies

  - Updated `globset` to 0.5.0
  ```

A member located at the repository root shares the root changelog instead of
getting a second file in the same place.

### Dependency updates

In an independent workspace, when a member's new version no longer satisfies a
dependent's version requirement, the dependent is released too, with a patch
bump. Its changelog section lists the dependency change under
`### Dependencies`, even if it has no commits of its own.

### Previewing and writing workspace changelogs

With `"per-member"` or `"both"`, the preview prints each file under a header so
you can check them before writing:

```bash
jj-release changelog
```

```text
==> CHANGELOG.md <==
## [0.11.0] - 2026-11-02
...
==> lib/CHANGELOG.md <==
## [0.11.0] - 2026-11-02
...
==> cli/CHANGELOG.md <==
...
```

```bash
# Prepend the next sections to every configured file
jj-release changelog -w

# Regenerate every configured file from history
jj-release changelog --full -w
```

<!--prettier-ignore-->
> [!NOTE]
> The preview doesn't include `### Dependencies` notes or members released only
> because of a dependency update; those are added during the release itself.
> With independent versioning, `--full` regenerates each member's changelog but
> not the aggregated root changelog.
