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
}
