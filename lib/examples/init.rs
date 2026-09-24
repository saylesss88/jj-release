//! Demonstrates how to generate a release.toml configuration programmatically.
//!
//! Run with: `cargo run --example init -- /path/to/repo`

use std::{env, path::PathBuf};
// use std::fs;

use jj_release_core::{config, errors::Result};

#[allow(clippy::unnecessary_wraps)]
fn main() -> Result<()> {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let release_toml = root.join("release.toml");
    if release_toml.exists() {
        println!("release.toml already exists. Delete it first to reinitialize.");
        return Ok(());
    }

    let content = config::generate_release_toml(root);
    println!("{content}");

    // Uncomment to write to disk:
    // fs::write(&release_toml, &content)?;
    // println!("✓ Written release.toml");

    Ok(())
}
