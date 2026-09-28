# Quick start

The fastest way to set up a new repo:

```sh
jj-release init     # detects forge, language, workspace: writes release.toml
jj-release validate # confirms everything is ready

# After adding releasable commits:
jj new -m "Release: please"
jj git push --bookmark main
```

`init` detects:

- Forge: from the git remote URL (`github.com` → github, `gitlab.com` → gitlab,
  `codeberg.org` → forgejo, `gitea.com` → gitea)
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
