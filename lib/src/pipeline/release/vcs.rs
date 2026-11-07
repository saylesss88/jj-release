use std::{fs, path::Path};

use semver::Version;
use toml_edit::DocumentMut;

use crate::{
    config::Config,
    errors::{ReleaseError, Result},
    manifest,
};

/// Synchronizes cross-member dependency versions within a Cargo workspace.
///
/// Iterates through all workspace members and updates any local dependencies
/// on other workspace members to match the upcoming release version.
///
/// # Errors
///
/// Returns an error if a member's `Cargo.toml` cannot be read, contains
/// invalid TOML syntax, or cannot be written back to disk.
pub fn update_workspace_dependencies(
    root: &Path,
    config: &Config,
    next_version: &Version,
) -> Result<()> {
    let Some(ws) = &config.workspace else {
        return Ok(());
    };
    if !ws.enabled {
        return Ok(());
    }

    let next_str = next_version.to_string();

    for member in &ws.members {
        let cargo_toml = root.join(&member.path).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }

        let raw = fs::read_to_string(&cargo_toml)?;
        let mut doc: DocumentMut = raw
            .parse()
            .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", cargo_toml.display())))?;

        for other in &ws.members {
            manifest::bump_workspace_dependency(&mut doc, &other.name, &next_str);
        }

        fs::write(&cargo_toml, doc.to_string())?;
    }

    Ok(())
}
