//! A simple example demonstrating how to use the `jj_release` library
//! to programmatically generate a changelog without invoking the CLI.
//!
//! Run this example with: `cargo run --example generate_changelog -- path/to/repo`
use std::env;
use std::path::PathBuf;

use jj_release_core::{
    config,
    errors::Result,
    forge::NoForge,
    jj::ShellBackend,
    manifest::detect_manifest,
    pipeline::{self, ChangelogOutput, ReleaseContext},
    publish::NoPublish,
};

fn main() -> Result<()> {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let backend = ShellBackend::new(root)?;
    let manifest = detect_manifest(root);
    let forge = NoForge;
    let publisher = NoPublish;
    let ctx = ReleaseContext::new(&backend, manifest.as_ref(), &forge, &publisher);
    let config = config::load(root)?;

    println!("Generating changelog for commits since the last tag...\n---\n");
    pipeline::print_changelog(&ctx, &config, root, ChangelogOutput::Stdout, false)?;
    // Actually create the CHANGELOG.md from the latest tag in /path/to/repo
    // pipeline::print_changelog(&ctx, &config, root, ChangelogOutput::Configured, false)?;

    println!("\n---\nDone.");

    Ok(())
}
