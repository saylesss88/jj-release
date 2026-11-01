//! Check if a release is pending and inspect the prepared release.
//!
//! Run with: `cargo run --example prepare_release -- /path/to/repo`

use std::env;
use std::path::PathBuf;

use jj_release_core::{ReleasePlan, config, errors::Result, jj::ShellBackend, manifest, pipeline};

fn main() -> Result<()> {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let backend = ShellBackend::new(root)?;
    let manifest = manifest::detect_manifest(root);
    let config = config::load(root)?;

    let Some(prepared) = pipeline::prepare_release(&backend, manifest.as_ref(), &config, root)?
    else {
        println!("No release pending.");
        return Ok(());
    };

    println!(
        "Release pending ({} commits since {}):",
        prepared.commits.len(),
        prepared.since
    );

    match &prepared.plan {
        ReleasePlan::Unified(plan) => {
            println!("  Current version: {}", plan.current_version);
            println!("  Next version:    {}", plan.next_version);
            println!("  Tag:             {}", plan.tag_name);
            println!("  Bump:            {:?}", plan.bump);
        }
        ReleasePlan::Independent(members) => {
            let mut names: Vec<_> = members.keys().collect();
            names.sort();
            for name in names {
                let plan = &members[name];
                if plan.first_release {
                    println!("  {name}: {} (first release)", plan.next);
                } else if plan.is_released() {
                    println!("  {name}: {} → {}", plan.current, plan.next);
                } else {
                    println!("  {name}: {} (no changes)", plan.current);
                }
            }
        }
    }
    Ok(())
}
