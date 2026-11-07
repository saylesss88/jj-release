use std::{path::Path, process::Command};

use semver::Version;

use crate::errors::{ReleaseError, Result};

/// Fetches the latest stable release tag for the given prefix (e.g., "v" or "my-crate-v").
///
/// # Errors
///
/// Returns an error if:
/// - The `jj` executable cannot be found or spawned by the system.
/// - The `jj tag list` command returns a non-zero exit status (e.g., if the
///   `root` directory is not a valid Jujutsu repository).
pub fn latest_release_tag(root: &Path, prefix: &str) -> Result<Option<Version>> {
    let out = Command::new("jj")
        .args(["tag", "list", "-T", r#"name ++ "\n""#])
        .current_dir(root)
        .output()
        .map_err(|e| ReleaseError::Message(format!("running jj tag list: {e}")))?;

    if !out.status.success() {
        return Err(ReleaseError::Message(format!(
            "jj tag list failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }

    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().strip_prefix(prefix))
        .filter_map(|v| Version::parse(v).ok())
        .filter(|v| v.pre.is_empty())
        .max())
}
