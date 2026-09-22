//! Detect forge, language, and versioning strategy for a repository.
//!
//! Run with: `cargo run --example detect_env -- /path/to/repo`

use jj_release_core::{config::Versioning, detect, workspace};
use std::{env, path::PathBuf};

fn main() {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();
    let forge = detect::detect_forge(root).unwrap_or("none");
    let language = detect::detect_language(root).unwrap_or("unknown");

    println!("Forge:    {forge}");
    println!("Language: {language}");

    if let Ok(Some(ws)) = workspace::detect_workspace(root) {
        println!("Workspace: Yes ({} members)", ws.members.len());

        let strategy = match ws.versioning {
            Versioning::Unified => "Unified",
            Versioning::Independent => "Independent",
        };
        println!("Strategy:  {strategy}");

        for member in &ws.members {
            println!("  - {} ({})", member.name, member.path);
        }
    } else {
        println!("Workspace: No");
        println!("Strategy:  Single Package");
    }
}
