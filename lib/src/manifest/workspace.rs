//! Cargo workspace backend.
//!
//! Reads and writes the shared version in `[workspace.package].version` of the
//! root `Cargo.toml`, preserving formatting via `toml_edit`. Member crates that
//! inherit it with `version.workspace = true` pick up the new version
//! automatically; members that declare their own `version` are not touched.

use std::{fs, path::Path};

use semver::Version;
use toml_edit::DocumentMut;

use crate::ManifestBackend;
use crate::errors::{ReleaseError, Result};

pub struct WorkspaceManifest;

impl ManifestBackend for WorkspaceManifest {
    fn read_version(&self, root: &Path) -> Result<Version> {
        let path = root.join("Cargo.toml");
        let raw = fs::read_to_string(&path)
            .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;
        let doc: DocumentMut = raw
            .parse()
            .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;
        let version_str = doc
            .get("workspace")
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ReleaseError::Message("missing [workspace.package].version".into()))?;
        Version::parse(version_str)
            .map_err(|e| ReleaseError::Message(format!("invalid semver {version_str:?}: {e}")))
    }

    fn write_version(&self, root: &Path, version: &Version) -> Result<()> {
        let path = root.join("Cargo.toml");

        let raw = fs::read_to_string(&path)
            .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;

        let mut doc: DocumentMut = raw
            .parse()
            .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", path.display())))?;

        doc["workspace"]["package"]["version"] = toml_edit::value(version.to_string());
        fs::write(&path, doc.to_string())
            .map_err(|e| ReleaseError::Message(format!("writing {}: {e}", path.display())))?;

        Ok(())
    }

    fn sync_lockfile(&self, root: &Path) -> Result<()> {
        super::cargo::sync_lockfile(root)
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn sync_lockfile_updates_members_inheriting_the_workspace_version() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n\n\
             [workspace.package]\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("member/src")).unwrap();
        fs::write(
            root.join("member/Cargo.toml"),
            "[package]\nname = \"member\"\nversion.workspace = true\nedition.workspace = true\n",
        )
        .unwrap();
        fs::write(root.join("member/src/lib.rs"), "").unwrap();

        let cargo = |args: &[&str]| {
            Command::new("cargo")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .status
                .success()
        };
        assert!(cargo(&["generate-lockfile", "--offline"]));
        let in_sync = || cargo(&["metadata", "--locked", "--offline", "--format-version", "1"]);
        assert!(in_sync());

        WorkspaceManifest
            .write_version(root, &Version::parse("0.2.0").unwrap())
            .unwrap();
        assert!(!in_sync(), "a version bump should leave the lock stale");

        WorkspaceManifest.sync_lockfile(root).unwrap();
        assert!(in_sync());
        let lock = fs::read_to_string(root.join("Cargo.lock")).unwrap();
        assert!(
            lock.contains("name = \"member\"\nversion = \"0.2.0\""),
            "{lock}"
        );
    }
}
