# Installation

```sh
# Latest changes from source
cargo install --git https://github.com/saylesss88/jj-release.git
# From crates.io
cargo install jj-release
```

Requires `jj` on your PATH.

Optional dependencies:

- For forge releases, `gh` or `glab` must also be available.
- For independent workspace versioning, `cargo-edit` must be installed.
- `cargo-semver-checks` (Required by default to prevent accidental breaking
  changes. Can be disabled by setting `semver_checks = false` in
  `release.toml`).

### NixOS

Try it out without installing permanently :

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

### Arch Linux

```bash
paru -S jj-release
```

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
