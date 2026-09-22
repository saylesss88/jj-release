//! Demonstrates how to generate a complete changelog from all tags.
//!
//! Run with: `cargo run --example full_changelog -- /path/to/repo`

use std::{env, path::PathBuf};

use jj_release_core::{
    config::Config,
    errors::Result,
    forge::NoForge,
    jj::ShellBackend,
    manifest,
    pipeline::{self, ReleaseContext},
    publish::NoPublish,
};

fn main() -> Result<()> {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let backend = ShellBackend::new(root)?;
    let manifest = manifest::detect_manifest(root);
    let forge = NoForge;
    let publisher = NoPublish;
    let ctx = ReleaseContext::new(&backend, manifest.as_ref(), &forge, &publisher);
    let config = Config::default();

    println!("Generating full changelog from all tags...\n");
    pipeline::print_full_changelog(&ctx, &config, root, None)?;

    Ok(())
}
