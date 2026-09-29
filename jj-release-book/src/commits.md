# Conventional Commits

`jj-release` follows the
[Conventional Commits](https://www.conventionalcommits.org) spec to determine
the version bump. It natively supports standard Angular types alongside a
dedicated `bug` type:

| Commit type                          | Bump       | Changelog Header   |
| ------------------------------------ | ---------- | ------------------ |
| `feat:`                              | minor      | Added              |
| `fix:`                               | patch      | Fixed              |
| `bug:`                               | patch      | Bug                |
| `refactor:`                          | patch      | Changed            |
| `perf:`                              | patch      | Performance        |
| `docs:`                              | No release | Documentation      |
| `feat!:` or `BREAKING CHANGE` footer | major      | Breaking           |
| `chore:`, `docs:`, `test:`, etc.     | No release | Dropped by default |
| `deprecate:`, `deprecated:`          | No release | Deprecated         |
| `chore:`, `test:`, `ci:`             | No release | Dropped by default |

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

