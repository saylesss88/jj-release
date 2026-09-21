//! A simple example demonstrating how to use the `jj_release` library
//! to programmatically generate a changelog without invoking the CLI.
//!
//! Run this example with: `cargo run --example generate_changelog`

use std::env;
use std::path::PathBuf;

use jj_release::{
    config::Config,
    errors::Result,
    forge::NoForge,
    jj::ShellBackend,
    manifest::CargoManifest,
    pipeline::{self, ReleaseContext},
    publish::NoPublish,
};

fn main() -> Result<()> {
    // 1. Accept a path argument or default to current dir
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);

    let root = root.as_path();

    // 2. Initialize the backend components
    // We use ShellBackend to interact with the local `jj` repository,
    // and stub out the forge and publisher since we only want to read data.
    let backend = ShellBackend::new(root)?;
    let manifest = CargoManifest;
    let forge = NoForge;
    let publisher = NoPublish;

    // 3. Assemble the release context
    let ctx = ReleaseContext::new(&backend, &manifest, &forge, &publisher);

    // 4. Load the configuration (this will fall back to defaults if release.toml is missing)
    let config = Config::default();

    println!("Generating changelog for commits since the last tag...\n");
    println!("---\n");

    // 5. Generate the changelog and print it directly to stdout
    // The arguments are: context, config, root path, output file (None = stdout), full history (false)
    pipeline::print_changelog(&ctx, &config, root, None, false)?;

    println!("\n---");
    println!("Changelog generated successfully!");

    Ok(())
}
