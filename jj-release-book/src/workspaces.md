# Workspaces

`jj-release` supports Rust workspaces with two versioning strategies. Run
`jj-release init` to auto-detect which one your workspace uses.

### Unified Versioning

All members share a single version from [workspace.package]. Every member bumps
together on each release:

```toml
[workspace]
enabled = true
versioning = "unified" # or "independent"

[[workspace.members]]
name = "mylib"
path = "lib"
publish = true

[[workspace.members]]
name = "mycli"
path = "cli"
publish = true
depends_on = ["mylib"]   # publish lib before cli
```

Cross-member dependency versions are updated automatically during the release.
You do not need to manually update `Cargo.toml` dependency versions between
workspace members before releasing.

For example, the `version` requirement in this local dependency will
automatically be kept in sync:

```toml
[dependencies]
jj_release_core = { path = "../lib", version = "0.7.0" }
```

### Independent Versioning

Each member has its own version and only bumps when commits touched its path.
Members get their own tag prefix:

```toml
[workspace]
enabled = true
versioning = "independent"

[[workspace.members]]
name = "mylib"
path = "lib"
publish = true
tag_prefix = "mylib-v"   # creates tags like mylib-v0.5.0

[[workspace.members]]
name = "mycli"
path = "cli"
publish = true
tag_prefix = "v"
depends_on = ["mylib"]
```

<!-- prettier-ignore -->
> [!NOTE]
> Independent versioning requires `cargo-edit` for version bumping:
>
> ```sh
> cargo install cargo-edit
> ```

Members are published in dependency order. `mylib` before `mycli`. So crates.io
has time to index the library before the CLI tries to depend on it.

```sh
jj-release --dry-run
 Current version: 0.7.0
[dry-run] Independent workspace release:
[dry-run] Would publish in order:
  - mylib (lib) 0.4.0 (no changes, skipping)
  - mycli (cli) 0.7.0 → 0.8.0 (tag: mycli-v0.8.0)
```

Cross-member dependency versions are updated automatically during the release.
You do not need to manually update `Cargo.toml` dependency versions between
workspace members before releasing.
