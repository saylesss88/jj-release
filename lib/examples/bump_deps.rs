//! Demonstrates updating cross-member dependency versions without a full release.
//!
//! Run with: `cargo run --example bump_deps -- /path/to/repo`

use jj_release_core::{
    ManifestBackend,
    config::Versioning,
    errors::{ReleaseError, Result},
    manifest::{self, WorkspaceManifest},
    workspace,
};
use std::{env, fs, path::PathBuf};
use toml_edit::DocumentMut;

fn main() -> Result<()> {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let Ok(Some(ws)) = workspace::detect_workspace(root) else {
        println!("No Cargo workspace detected at {}", root.display());
        return Ok(());
    };

    if matches!(ws.versioning, Versioning::Independent) {
        println!("Independent workspace: per-member versions, nothing to bump uniformly.");
        return Ok(());
    }

    let version = WorkspaceManifest.read_version(root)?;
    println!("Bumping cross-member deps to {version}");

    for member in &ws.members {
        if member.depends_on.is_empty() {
            continue;
        }

        let cargo_toml = root.join(&member.path).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }

        let raw = fs::read_to_string(&cargo_toml)?;
        let mut doc: DocumentMut = raw
            .parse()
            .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", cargo_toml.display())))?;

        for dep in &member.depends_on {
            manifest::bump_workspace_dependency(&mut doc, dep, &version.to_string());
            println!("  {} → updated dep on {dep}", member.name);
        }

        fs::write(&cargo_toml, doc.to_string())?;
    }

    Ok(())
}
