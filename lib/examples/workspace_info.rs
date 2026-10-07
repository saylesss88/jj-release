//! Show workspace members, versions, and topological publish order.
//!
//! Run with: `cargo run --example workspace_info -- /path/to/repo`

use jj_release_core::{ManifestBackend, config::Versioning, manifest, workspace};
use std::{env, path::PathBuf};

fn main() {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let Ok(Some(ws)) = workspace::detect_workspace(root) else {
        println!("No Cargo workspace detected at {}", root.display());
        return;
    };
    let strategy = match ws.versioning {
        Versioning::Unified => "Unified",
        Versioning::Independent => "Independent",
    };
    println!("Versioning: {strategy}");
    println!("Members:    {}", ws.members.len());

    // Per-member versions
    println!("\nCurrent versions:");
    for member in &ws.members {
        let member_root = root.join(&member.path);
        // Try member's own Cargo.toml first but fall back to workspace root
        let version = manifest::CargoManifest
            .read_version(&member_root)
            .or_else(|_| manifest::WorkspaceManifest.read_version(root));
        match version {
            Ok(v) => println!("  {} ({}) — {v}", member.name, member.path),
            Err(e) => println!("  {} ({}) — error: {e}", member.name, member.path),
        }
    }

    // Topological publish order
    println!("\nPublish order:");
    match workspace::ordered_members(&ws.members) {
        Ok(ordered) => {
            for (i, member) in ordered.iter().enumerate() {
                let deps = if member.depends_on.is_empty() {
                    String::new()
                } else {
                    format!(" (depends on: {})", member.depends_on.join(", "))
                };
                println!("  {}. {}{}", i + 1, member.name, deps);
            }
        }
        Err(e) => println!("  error computing order: {e}"),
    }
}
