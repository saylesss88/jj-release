# jj-release

<p align="center">
  <img src="https://raw.githubusercontent.com/saylesss88/jj-release/main/assets/jj-release-lockup.svg"
       alt="jj-release: semantic releases & changelogs for jj" width="400">
</p>

<p align="center">
  <a href="https://crates.io/crates/jj-release"><img src="https://img.shields.io/crates/v/jj-release.svg" alt="crates.io"></a>
  <a href="https://docs.rs/jj_release_core"><img src="https://img.shields.io/docsrs/jj_release_core" alt="docs.rs"></a>
  <a href="https://aur.archlinux.org/packages/jj-release"><img src="https://img.shields.io/aur/version/jj-release" alt="AUR"></a>
  <a href="LICENSE"><img src="https://img.shields.io/crates/l/jj-release.svg" alt="License"></a>
</p>

Automated semantic releases and changelogs for
[Jujutsu](https://github.com/jj-vcs/jj) repositories.

Tools like `release-plz` and `cargo-release` are built around mutable Git
branches. `jj-release` is built for `jj`: it walks history with revsets, moves
bookmarks instead of branches, and starts a release from a commit message, so
shipping is just another `jj new`.

**[Read the book →](https://saylesss88.github.io/jj-release-book/)**

## Features

- **Semantic versioning from your commits.** Bumps are computed from
  [Conventional Commits](https://www.conventionalcommits.org), with
  [cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks)
  catching breaking API changes your commit messages missed.
- **Keep a Changelog output.** Grouped, scoped, optionally emoji-headed sections
  prepended to `CHANGELOG.md`.
- **Forge releases and PRs** on GitHub, GitLab, Forgejo/Codeberg, and Gitea.
- **Workspaces** with unified or independent versioning, published in dependency
  order.
- **Version bumps everywhere.** Manifests, cross-member dependencies,
  `flake.nix`, and any file you configure with regex replacements.
- **Distro packages.** Optionally update your AUR package and submit a Fedora
  COPR build after each release.
- **Safe by default.** Pre-flight checks run before anything changes, and
  nothing is pushed until the publish succeeds.

Rust is the primary target. npm and Go manifests are supported but less
battle-tested.

## Installation

```sh
cargo install jj-release
```

| Platform    | Command                                                                    |
| ----------- | -------------------------------------------------------------------------- |
| Arch (AUR)  | `paru -S jj-release`                                                       |
| Fedora      | `sudo dnf copr enable sayless88/jj-release && sudo dnf install jj-release` |
| Nix         | `nix run github:saylesss88/jj-release`                                     |
| From source | `cargo install --git https://github.com/saylesss88/jj-release`             |

Requires `jj` 0.43+ in a colocated repository (`jj git init --colocate`). Forge
releases need `gh` or `glab` for GitHub and GitLab. See
[Installation](https://saylesss88.github.io/jj-release/installation.html) for
Cargo features, NixOS configuration, and optional tools.

## Quick start

```sh
jj-release init       # detect forge, language and workspace; write release.toml
jj-release validate   # check that everything is ready

# When you're ready to ship:
jj new -m "Release: please"
jj-release --dry-run  # preview
jj-release            # release
```

`jj-release` runs locally or in CI. For GitHub Actions, see
[CI setup](https://saylesss88.github.io/jj-release/ci.html).

## How it works

When `jj-release` finds a commit containing the trigger (`Release: please` by
default) since the last version tag, it:

1. Classifies commits since the last tag as major, minor, or patch, using the
   latest crates.io version as the baseline.
2. Runs pre-flight checks, including `cargo publish --dry-run`.
3. Writes the changelog and bumps versions.
4. Creates a `chore: release vX.Y.Z` commit and tag.
5. Publishes to crates.io.
6. Moves the bookmark and pushes the commit and tag.
7. Creates a forge release and updates distro packages, if configured.

If anything fails before the push, your remote is untouched. See
[Recovering from a failed release](https://saylesss88.github.io/jj-release/recovery.html).

Prefer to review first? `jj-release pr` opens a pull request with the changelog
preview instead of releasing.

## Documentation

- [Configuration reference](https://saylesss88.github.io/jj-release/configuration.html)
- [Conventional Commits and changelogs](https://saylesss88.github.io/jj-release/commits.html)
- [Forges](https://saylesss88.github.io/jj-release/forges.html)
- [Workspaces](https://saylesss88.github.io/jj-release/workspaces.html)
- [AUR](https://saylesss88.github.io/jj-release/aur.html) and
  [COPR](https://saylesss88.github.io/jj-release/copr.html) publishing
- [Library API (`jj_release_core`)](https://docs.rs/jj_release_core)

## Repository layout

- [`cli/`](cli/): the `jj-release` binary
- [`lib/`](lib/): `jj_release_core`, the library behind it

## Credits

Ideas and code adapted from
[release-plz](https://github.com/release-plz/release-plz) (crates.io version
baseline, semver checks),
[cargo-release](https://github.com/crate-ci/cargo-release), and
[git-cliff](https://github.com/orhun/git-cliff) (changelogs).

## License

[Apache-2.0](LICENSE)
