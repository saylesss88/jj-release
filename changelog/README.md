# jj_release_changelog

Render [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) sections from
[Conventional Commits](https://www.conventionalcommits.org/) messages.

Pure and VCS-agnostic: give it commit messages, get markdown back. No git, jj,
filesystem, or network access. It's the renderer behind
[`jj-release`](https://github.com/saylesss88/jj-release), but works with commits
from any source.

## Usage

```toml
[dependencies]
jj_release_changelog = "0.11"
# Deserialize ChangelogConfig from TOML/JSON:
# jj_release_changelog = { version = "0.11", features = ["serde"] }
```

```rust
use jj_release_changelog::{ChangelogConfig, CommitInfo, render_changelog_section};
use semver::Version;

let commits = vec![
    CommitInfo::from("feat(cli): add --write flag"),
    CommitInfo::from("fix: handle empty files"),
];
let section = render_changelog_section(
    &commits,
    Some(&Version::new(0, 11, 0)),
    &ChangelogConfig::default(),
    None,
);
```

## What it does

- Groups commits into Added, Changed, Fixed, Performance, Deprecated, and more
- Marks breaking changes (`feat!:`) and bolds scopes (`feat(cli):`)
- Optional emoji headers, custom prefix → header mappings, excluded prefixes
- Strict mode (default) drops non-conventional commits; non-strict keeps them
  under "Other Changes"
- Compare-URL links on version headings for GitHub, GitLab, Forgejo, and Gitea
- `prepend_to_file` inserts a new section while preserving custom header text
