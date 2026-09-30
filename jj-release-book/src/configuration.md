# Configuration

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
cargo = false                        # set to true to publish to crates.io
cargo_flags = []                     # extra flags forwarded to `cargo publish`
semver_checks = true                 # run cargo-semver-checks before publishing
semver_checks_upgrade_major = false  # auto-upgrade to major (default: only post-1.0)

[changelog]
enabled = true                       # write a CHANGELOG.md entry on each release
file = "CHANGELOG.md"                # path to the changelog file
strict = true                        # drop unmapped/unformatted commits (false = "Other Changes")

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

manifest_backend = "cargo"           # manifest format: cargo, npm, go
```

By default, `jj-release` only handles versioning, changelogs, tags, and forge
releases. To automatically publish to crates.io, explicitly enable it in your
configuration:

```toml
[publish]
cargo = true
```

Publishing can also be skipped per-run with `--no-publish` without editing
`release.toml`, useful for testing the release pipeline or in scripts that
handle publishing separately.

### Changelog Configuration

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
