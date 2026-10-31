//! Demonstrates how to generate a complete changelog from all tags.
//!
//! Run with: `cargo run --example full_changelog -- /path/to/repo`

use std::{env, path::PathBuf};

use jj_release_core::{
    config,
    errors::Result,
    forge::NoForge,
    jj::ShellBackend,
    manifest,
    pipeline::{self, ChangelogOutput, ReleaseContext},
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
    let config = config::load(root)?;

    println!("Generating full changelog from all tags...\n");
    // Print the full changelog to stdout
    pipeline::print_full_changelog(&ctx, &config, root, ChangelogOutput::Stdout)?;
    // Actually create the CHANGELOG.md in the /path/to/repo
    // pipeline::print_full_changelog(&ctx, &config, root, ChangelogOutput::Configured)?;

    Ok(())
}
