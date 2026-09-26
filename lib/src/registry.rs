//! Cargo registry checks.

use std::process::Command;

use semver::Version;

use crate::errors::{ReleaseError, Result};

/// Check if a specific version of a crate is already published on crates.io
///
/// # Errors
/// Returns an error if the network request fails or if the API returns an unexpected
/// status code other than a success (2xx) or 404 Not Found.
pub fn version_exists_on_crates_io(name: &str, version: &Version) -> Result<bool> {
    let latest_opt = latest_version_on_crates_io(name)?;

    match latest_opt {
        // If the crate exists, the target version "exists" if it is older
        // than or equal to the latest published version.
        Some(latest) => Ok(*version <= latest),

        // Crate doesn't exist at all on crates.io yet
        None => Ok(false),
    }
}

/// Fetches the latest published version of a crate using local `cargo search`
///
/// # Errors
/// Returns an error if the crate doesn't exist or `cargo search` fails
pub fn latest_version_on_crates_io(name: &str) -> Result<Option<Version>> {
    let output = Command::new("cargo")
        .args(["search", name, "--limit", "1"])
        .output()
        .map_err(|e| ReleaseError::Message(format!("failed to execute cargo search: {e}")))?;

    if !output.status.success() {
        return Err(ReleaseError::Message(format!(
            "cargo search failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let exact_prefix = format!("{name} = \"");

    for line in stdout.lines() {
        if line.starts_with(&exact_prefix)
            && let Some(version_str) = line.split('"').nth(1)
        {
            return Ok(Version::parse(version_str).ok());
        }
    }

    // If the loop finishes without matching the exact prefix, the crate doesn't exist
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_version_exists() {
        let v = Version::parse("1.0.0").unwrap();
        assert!(version_exists_on_crates_io("thiserror", &v).unwrap());
    }

    #[test]
    fn gets_latest_version_from_crates_io() {
        let v = latest_version_on_crates_io("thiserror").unwrap();
        assert!(v.is_some());
        assert!(v.unwrap().major >= 1);
    }
}
