//! Check if a release is pending and inspect the prepared release.
//!
//! Run with: `cargo run --example prepare_release -- /path/to/repo`

use std::env;
use std::path::PathBuf;

use jj_release_core::{config::Config, errors::Result, jj::ShellBackend, manifest, pipeline};

fn main() -> Result<()> {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let backend = ShellBackend::new(root)?;
    let manifest = manifest::detect_manifest(root);
    let config = Config::default();

    match pipeline::prepare_release(&backend, manifest.as_ref(), &config, root)? {
        Some(prepared) => {
            println!("Release pending:");
            println!("  Current version: {}", prepared.current_version);
            println!("  Next version:    {}", prepared.next_version);
            println!("  Tag:             {}", prepared.tag_name);
            println!("  Bump:            {:?}", prepared.bump);
            println!("  Commits:         {}", prepared.commits.len());
        }
        None => println!("No release pending."),
    }
    Ok(())
}
