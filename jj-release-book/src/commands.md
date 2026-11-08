# Commands

```sh
# Show usage and help menu
jj-release --help
# Auto-detect environment and generate a release.toml
jj-release init

# Check everything is ready before releasing
jj-release validate

# Full release pipeline
jj-release

# Dry run, see what would happen without changing anything
jj-release --dry-run

# Run release without publishing to crates.io
jj-release --no-publish

# Force publishing to crates.io even if publish.cargo = false
jj-release -p

# Print the next version that would be released
jj-release next-version

# Override next bump
jj-release --bump major

# Release candidates
jj-release --bump major --pre rc   # 1.0.0-rc.1
jj-release --pre rc                # 1.0.0-rc.2
jj-release                         # 1.0.0

# Preview the next changelog section
jj-release changelog

# Prepend the next section to an existing CHANGELOG.md
jj-release changelog -o CHANGELOG.md

# Regenerate the full changelog from all tags (useful for bootstrapping)
jj-release changelog --full -o CHANGELOG.md

# Choose a specific changelog layout (root, per-member, or both)
jj-release changelog --layout both

# Open a PR for review before publishing
jj-release pr
```

For a more integrated feel, you can add the following to your `~/.config/jj/config.toml`:

```toml
[aliases]
release = ["util", "exec", "--", "jj-release"]
```

Now `jj release` will work to run `jj-release`.
