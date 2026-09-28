# CI setup

## GitHub Actions

<!-- prettier-ignore -->
> [!NOTE]
> This action takes a while to finish since it compiles `jj` and `jj-release`
> from source. Use `jj-release` locally if you're in a hurry.

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

    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
          token: ${{ secrets.GITHUB_TOKEN }}

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            ~/.cargo/bin
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: |
            ${{ runner.os }}-cargo-

      - name: Install jj
        run: cargo install jj-cli --locked

      - name: Install jj-release
        run: cargo install jj-release --locked

      - name: Attach jj to colocated git repo
        run: jj git init --colocate

      - name: Run release
        run: jj-release
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

