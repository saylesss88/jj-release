//! Read and write project versions across manifest formats.
//!
//! - [`CargoManifest`]: `Cargo.toml`, edited with `toml_edit` to preserve formatting
//! - [`NpmManifest`][]: `package.json`
//! - [`GoManifest`]: no manifest version; Go modules are versioned by tag
//! - [`WorkspaceManifest`]: Cargo workspaces

mod cargo;
mod go;
mod npm;
mod workspace;

use std::path::Path;

use semver::Version;

pub use cargo::{CargoManifest, bump_workspace_dependency, read_name};
pub use go::GoManifest;
pub use npm::NpmManifest;

pub use workspace::WorkspaceManifest;
// Kept for API compatibility: these moved to `crate::bump`.
pub use crate::bump::{apply_replacements, bump_flake_nix};

use crate::errors::Result;

/// Manages reading and writing version information in project manifests.
///
/// # Example
///
/// ```no_run
/// use jj_release_core::manifest::{ManifestBackend, CargoManifest};
/// use semver::Version;
/// use std::path::Path;
///
/// let root = Path::new(".");
///
/// let current = manifest.read_version(root).unwrap();
/// println!("Current version: {current}");
///
/// let next = Version::new(current.major, current.minor + 1, 0);
/// manifest.write_version(root, &next).unwrap();
/// ```
pub trait ManifestBackend {
    /// Reads the current version from the project manifest at the given root path.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest file cannot be found, cannot be read due
    /// to permissions, or contains invalid syntax that prevents version extraction.
    fn read_version(&self, root: &Path) -> Result<Version>;

    /// Writes the specified version to the project manifest at the given root path.
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest file cannot be read or written due to
    /// permissions, or if serialization of the new version fails.
    fn write_version(&self, root: &Path, version: &Version) -> Result<()>;
}

/// Detect and return the appropriate manifest backend for the given root.
/// Returns `WorkspaceManifest` for Rust workspaces, `NpmManifest` for npm,
/// `GoManifest` for Go, and `CargoManifest` as the default.
#[must_use]
pub fn detect_manifest(root: &Path) -> Box<dyn ManifestBackend> {
    let cargo_toml = root.join("Cargo.toml");
    if cargo_toml.exists() {
        // Parse instead of substring-matching so a commented-out
        // `# [workspace]` doesn't count.
        if let Ok(doc) = cargo::load_doc(&cargo_toml)
            && doc.contains_key("workspace")
        {
            return Box::new(WorkspaceManifest);
        }
        return Box::new(CargoManifest);
    }
    if root.join("package.json").exists() {
        return Box::new(NpmManifest);
    }
    if root.join("go.mod").exists() {
        return Box::new(GoManifest);
    }
    Box::new(CargoManifest)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn detects_workspace_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.3.0\"\n",
        )
        .unwrap();
        // CargoManifest would fail here (no [package]), so a successful read
        // means the workspace backend was picked.
        let v = detect_manifest(dir.path())
            .read_version(dir.path())
            .unwrap();
        assert_eq!(v, Version::parse("0.3.0").unwrap());
    }

    #[test]
    fn commented_workspace_is_not_a_workspace() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "# [workspace]\n[package]\nname = \"test\"\nversion = \"0.4.0\"\n",
        )
        .unwrap();
        let v = detect_manifest(dir.path())
            .read_version(dir.path())
            .unwrap();
        assert_eq!(v, Version::parse("0.4.0").unwrap());
    }

    #[test]
    fn detects_npm_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name":"test","version":"1.0.0"}"#,
        )
        .unwrap();
        let v = detect_manifest(dir.path())
            .read_version(dir.path())
            .unwrap();
        assert_eq!(v, Version::parse("1.0.0").unwrap());
    }

    #[test]
    fn detects_cargo_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname=\"test\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        )
        .unwrap();
        let v = detect_manifest(dir.path())
            .read_version(dir.path())
            .unwrap();
        assert_eq!(v, Version::parse("0.1.0").unwrap());
    }
}
