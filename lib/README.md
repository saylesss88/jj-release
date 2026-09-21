# jj_release_core (Library)

The core automation engine behind `jj-release`. This crate provides a
programmatic, trait-driven Rust API for interacting with Jujutsu (`jj`), parsing
commits, resolving Semantic Versions, and orchestrating release pipelines.

## Architecture

The library is built around a functional core that orchestrates releases using a
set of pluggable backends. All side effects are abstracted behind traits, making
it highly testable and extensible.

To run a release pipeline, you construct a `ReleaseContext` containing
implementations for four core traits:

- **`JjBackend`**: Interfaces with the Jujutsu VCS (e.g., `ShellBackend`).
- **`ManifestBackend`**: Reads and writes project versions (e.g.,
  `CargoManifest`, `NpmManifest`).
- **`PublishBackend`**: Handles pushing artifacts to package registries.
- **`ForgeBackend`**: Interacts with remote Git forges like GitHub, GitLab, or
  Forgejo.

## Usage

Add `jj_release_core` to your `Cargo.toml`:

```toml
[dependencies]
jj_release_core = "0.1.0"
```

## Quick Start

```rust
use std::path::Path;

use jj_release_core::{
    config::Config,
    forge::NoForge,
    jj::ShellBackend,
    manifest,
    pipeline::{self, ReleaseContext},
    publish::NoPublish,
    errors::Result,
};

fn main() -> Result<()> {
    let root = Path::new(".");
    let backend = ShellBackend::new(root)?;
    let manifest = manifest::detect_manifest(root);
    let ctx = ReleaseContext::new(&backend, manifest.as_ref(), &NoForge, &NoPublish);
    let config = Config::default();

    // Generate a changelog preview
    pipeline::print_changelog(&ctx, &config, root, None, false)?;
    Ok(())
}
```

<!-- prettier-ignore -->
> [!NOTE]
> `ShellBackend` requires `jj` on PATH.

## Error Handling

All fallible operations return `jj_release::errors::Result<T>`, an alias for
`Result<T, ReleaseError>`. The `ReleaseError` enum is powered by `thiserror` and
covers IO, parsing, network, and release-specific failures, making it easy to
pattern-match against specific failure states in your own applications.

## Extending

Implement any of the core traits to customize behavior:

```rust
use jj_release_core::manifest::ManifestBackend;
use jj_release_core::errors::Result;
use semver::Version;

struct MyManifest;

impl ManifestBackend for MyManifest {
    fn read_version(&self, root: &std::path::Path) -> Result<Version> {
        // read version from your custom format
        todo!()
    }
    fn write_version(&self, root: &std::path::Path, version: &Version) -> Result<()> {
        // write version to your custom format
        todo!()
    }
}
```

---

## Examples

See the [`examples/`](examples/) directory for runnable demos:

- `generate_changelog`: generate a changelog section for any repo
- `next_version`: compute the next release version
- `validate_repo`: run preflight checks before releasing
