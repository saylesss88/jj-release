//! Demonstrates how to compute the next release version programmatically.
//!
//! Run with: `cargo run --example next_version -- /path/to/repo`

use std::path::PathBuf;

use jj_release::{
    config::Config,
    errors::Result,
    forge::NoForge,
    jj::ShellBackend,
    manifest,
    pipeline::{self, ReleaseContext},
    publish::NoPublish,
};

fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let backend = ShellBackend::new(root)?;
    let manifest = manifest::detect_manifest(root);
    let forge = NoForge;
    let publisher = NoPublish;
    let ctx = ReleaseContext::new(&backend, manifest.as_ref(), &forge, &publisher);
    let config = Config::default();

    pipeline::print_next_version(&ctx, &config, root)?;
    Ok(())
}
