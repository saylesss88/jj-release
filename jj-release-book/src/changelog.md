# Changelogs

`jj-release` follows the [Keep a Changelog](https://keepachangelog.com) format.
Each release prepends a new section to `CHANGELOG.md`:

```bash
# Preview the next changelog section
jj-release changelog

# Prepend the next section to an existing CHANGELOG.md
jj-release changelog -o CHANGELOG.md

# Regenerate the full changelog from all tags (useful for bootstrapping)
jj-release changelog --full -o CHANGELOG.md
```

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

## Changelog Configuration

You can customize how `jj-release` parses your commits and maps them to the Keep
a Changelog format. By default, only `feat`, `fix`, `bug`, `perf`, `refactor`,
`removed`, `security`, and `docs` commits are included.

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

# An array of commit types or prefixes to silently exclude from the changelog.
# Matches against the parsed conventional type or the raw string.
exclude_prefixes = ["chore", "ci", "test", "WIP:"]

# Override specific default emojis (keys must match the exact Header name)
[changelog.emoji_mapping]
Added = "🚀"
Fixed = "✅️"
Bug = "🔥"
Performance = "⚡️"
Testing = "🧪"

# Map conventional commit types to custom Keep a Changelog headers.
# Standard types (feat -> Added, fix -> Fixed, etc.) are handled automatically.
[changelog.prefix_mapping]
revert = "Reverted"
test = "Testing"
```
