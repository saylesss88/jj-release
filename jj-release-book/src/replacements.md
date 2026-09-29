# Version replacements

`jj-release` can automatically update version strings in arbitrary files (like
your `README.md`, installation scripts, or `flake.nix`) during the release
process.

## Native Nix Flake Support

If your repository contains a `flake.nix` file in the root directory with a
standard `version = "..."` declaration, `jj-release` will automatically detect
it and bump the version alongside your project manifest.

**This behavior is native and requires zero configuration.**

---

### Custom Replacements

You can configure additional regex-based search-and-replace operations across
other project files by adding an array of `[[replacements]]` tables to your
`release.toml`.

The `replace` field supports built-in template variables that are substituted
before the regex substitution occurs.

| Variable         | Description                              | Example Output |
| :--------------- | :--------------------------------------- | :------------- |
| `{{version}}`    | The new semantic version being released. | `1.2.3`        |
| `{{crate_name}}` | The name of the primary crate.           | `my_crate`     |
| `{{date}}`       | Today's date in `YYYY-MM-DD` format.     | `2026-09-24`   |

#### Configuration Example

```toml
# Update installation instructions in README.md
[[replacements]]
file = "README.md"
search = 'jj-release = ".*"'
replace = 'jj-release = "{{version}}"'

# Update version constants in source code, using capture groups
# for complex lines. Use native TOML braces {{1}} for regex groups.
[[replacements]]
file = "lib/src/manifest.rs"
search = 'version\s*=\s*"[^"]+"'
# NOTE: Native TOML braces become standard braces in regex replacement
replace = 'version = "${{1}}{{version}}${{3}}"'
```
