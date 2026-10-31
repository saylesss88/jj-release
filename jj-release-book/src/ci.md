# CI setup

## GitHub Actions

<!-- prettier-ignore -->
> [!NOTE]
> This workflow installs `jj` and `jj-release` as prebuilt binaries with
> `cargo binstall`, so it runs in about a minute plus your build time. If a
> prebuilt binary isn't available for the runner, binstall falls back to
> compiling from source, which takes much longer.

Add this workflow to your consumer repo at `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    branches:
      - main

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      actions: write

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
          token: ${{ secrets.GITHUB_TOKEN }}

      # Still needed: `cargo publish` and cargo-semver-checks run during the release.
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: |
            ${{ runner.os }}-cargo-

      - name: Install cargo-binstall
        uses: cargo-bins/cargo-binstall@main

      - name: Install jj, jj-release and cargo-semver-checks
        run:
          cargo binstall --no-confirm --disable-telemetry jj-cli jj-release
          cargo-semver-checks

      - name: Attach jj to colocated git repo
        run: jj git init --colocate

      - name: Run release
        run: jj-release
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

      - name: Build release binaries
        if: success()
        run: |
          tag=$(git describe --tags --abbrev=0)
          gh workflow run release_binaries.yml -f tag="$tag"
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

<!-- prettier-ignore -->
> [!NOTE]
> The last step triggers `release_binaries.yml` (see
> [Cargo Binstall support](#cargo-binstall-support-for-your-crate) below).
> If you don't publish prebuilt binaries, delete that step and remove
> `actions: write` from `permissions`.

## Cargo Binstall support for your Crate

Add the following to your `Cargo.toml`:

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }{ archive-suffix }"
pkg-fmt = "tgz"
```

Create a `.github/workflows/release_binaries.yml`:

```yaml
name: Release binaries

# Builds prebuilt binaries for `cargo binstall` and attaches them to a release.
#
# Triggers:
#   - release: published    -> only fires if the release was created with a PAT or
#                              GitHub App token. Releases made with GITHUB_TOKEN
#                              don't trigger other workflows.
#   - workflow_dispatch     -> run it by hand, or call it with
#                              `gh workflow run release_binaries.yml -f tag=vX.Y.Z`
#                              from your release workflow (needs `actions: write`).
on:
  release:
    types: [published]
  workflow_dispatch:
    inputs:
      tag:
        description: "Release tag to build and upload to (e.g. v2.0.0)"
        required: true
      unlocked:
        description: "Skip --locked (only for old tags with a stale Cargo.lock)"
        type: boolean
        default: false

permissions:
  contents: write

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { target: x86_64-unknown-linux-musl, os: ubuntu-latest }
          - { target: aarch64-apple-darwin, os: macos-latest }
          - { target: x86_64-apple-darwin, os: macos-latest }
          - { target: x86_64-pc-windows-msvc, os: windows-latest }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
        with:
          # Build the tagged source, whichever way the workflow was started.
          ref: ${{ inputs.tag || github.event.release.tag_name }}

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      # --locked fails the build if Cargo.lock doesn't match Cargo.toml, so the
      # binary uses exactly the dependency versions you committed. A published
      # release and a normal manual run both get it; only an explicit
      # `unlocked` opt-out skips it.
      - name: Build
        run:
          cargo build --release --target ${{ matrix.target }} ${{
          inputs.unlocked != true && '--locked' || '' }}

      - name: Package and upload
        shell: bash
        env:
          GH_TOKEN: ${{ github.token }}
          TAG: ${{ inputs.tag || github.event.release.tag_name }}
          TARGET: ${{ matrix.target }}
        run: |
          # Single-crate repos: the package name is read from Cargo. In a
          # workspace, set `name` to your binary crate by hand.
          name=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')
          ext=""; [[ "$TARGET" == *windows* ]] && ext=".exe"
          cp "target/$TARGET/release/$name$ext" .
          tar czf "$name-$TARGET.tar.gz" "$name$ext"
          gh release upload "$TAG" "$name-$TARGET.tar.gz" --clobber
```
