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
