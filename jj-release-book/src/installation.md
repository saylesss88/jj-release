# Installation

```sh
# Latest changes from source
cargo install --git https://github.com/saylesss88/jj-release.git
# From crates.io
cargo install jj-release
```

Requires `jj` on your PATH.

---

## Features

`jj-release` uses Cargo features to keep the binary lean, only compile in
support for the forges and publish targets you actually use.

## Default Features

The default build includes GitHub and GitLab support:

```sh
cargo install jj-release
```

## Forge Features

| Feature    | Enables                        | Requires     |
| ---------- | ------------------------------ | ------------ |
| `github`   | GitHub releases and PRs via `gh` | `gh` CLI   |
| `gitlab`   | GitLab releases and MRs via `glab` | `glab` CLI |
| `gitea`    | Gitea releases and PRs via REST API | `GITEA_TOKEN` |
| `forgejo`  | Forgejo/Codeberg via REST API (includes `gitea`) | `FORGEJO_TOKEN` |

```sh
# Forgejo/Codeberg only, no GitHub or GitLab
cargo install jj-release --no-default-features --features forgejo
```

## Publish Features

| Feature        | Enables                          |
| -------------- | -------------------------------- |
| `publish-aur`  | Publishing to the Arch User Repository |
| `publish-copr` | Publishing to Fedora COPR        |

```sh
# Full build with all forges and publish targets
cargo install jj-release --features full
```

## Library Features

If you're using `jj_release_core` as a library:

```toml
[dependencies]
# Default: GitHub + GitLab
jj_release_core = "0.9.0"

# Forgejo/Codeberg support
jj_release_core = { version = "0.9.0", features = ["forgejo"] }

# Everything
jj_release_core = { version = "0.9.0", features = ["full"] }
```

---

### NixOS

Try it out without installing permanently:

```bash
nix run github:saylesss88/jj-release
```

Flake input:

```nix
# In your flake.nix:
inputs.jj-release.url = "github:saylesss88/jj-release";

# In your configuration.nix (assuming `inputs` is passed via specialArgs)
{ inputs, pkgs, ... }: {
inputs.jj-release.packages.${pkgs.stdenv.hostPlatform.system}.default
}
```

> By default, the flake includes all features.

---

### Arch Linux

```bash
paru -S jj-release
```

---

### Fedora

You can install `jj-release` from the unofficial COPR repository. This
repository provides pre-compiled binaries built automatically for Fedora
systems.

First, enable the repository:

```bash
sudo dnf copr enable sayless88/jj-release
```

Then install the package:

```bash
sudo dnf install jj-release
```
