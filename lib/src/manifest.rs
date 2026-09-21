//! Edit `Cargo.toml` version fields without destroying formatting.

use std::{borrow::ToOwned, fs, path::Path};

use semver::Version;
use serde_json::Value;
use toml_edit::DocumentMut;

use crate::errors::{ReleaseError, Result};

/// Manages reading and writing version information in project manifests.
///
/// # Example
///
/// ```no_run
/// use jj_release::manifest::{ManifestBackend, CargoManifest};
/// use semver::Version;
/// use std::path::Path;
///
/// let manifest = CargoManifest;
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

pub struct CargoManifest;
pub struct GoManifest;
pub struct NpmManifest;

impl ManifestBackend for CargoManifest {
    fn read_version(&self, root: &Path) -> Result<Version> {
        read_version(&root.join("Cargo.toml"))
    }

    fn write_version(&self, root: &Path, version: &Version) -> Result<()> {
        write_version(&root.join("Cargo.toml"), version)
    }
}

impl ManifestBackend for GoManifest {
    fn read_version(&self, _root: &Path) -> Result<Version> {
        // Go modules are versioned by tag only, no manifest version field.
        // Return 0.0.0 so the bump logic computes from scratch.
        Ok(Version::new(0, 0, 0))
    }

    fn write_version(&self, _root: &Path, _version: &Version) -> Result<()> {
        Ok(())
    }
}

impl ManifestBackend for NpmManifest {
    fn read_version(&self, root: &Path) -> Result<Version> {
        let path = root.join("package.json");
        let raw = fs::read_to_string(&path)
            .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;
        let json: Value = serde_json::from_str(&raw)
            .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", path.display())))?;
        let version_str = json["version"]
            .as_str()
            .ok_or_else(|| ReleaseError::Message("missing version field in package.json".into()))?;
        Version::parse(version_str).map_err(|_| {
            ReleaseError::Message(format!("invalid semver {version_str:?} in package.json"))
        })
    }

    fn write_version(&self, root: &Path, version: &Version) -> Result<()> {
        let path = root.join("package.json");
        let raw = fs::read_to_string(&path)
            .map_err(|e| ReleaseError::Message(format!("writing {}: {e}", path.display())))?;

        let mut json: Value = serde_json::from_str(&raw)
            .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", path.display())))?;

        json["version"] = Value::String(version.to_string());
        fs::write(&path, serde_json::to_string_pretty(&json)?)
            .map_err(|e| ReleaseError::Message(format!("writing {}: {e}", path.display())))?;

        Ok(())
    }
}

/// Read the current `[package].version` from `Cargo.toml`.
pub(crate) fn read_version(cargo_toml: &Path) -> Result<Version> {
    let raw = fs::read_to_string(cargo_toml)
        .map_err(|_| ReleaseError::Message(format!("reading {}", cargo_toml.display())))?;

    let doc: DocumentMut = raw
        .parse()
        .map_err(|_| ReleaseError::Message(format!("parsing {}", cargo_toml.display())))?;

    let version_str = doc
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ReleaseError::Message(format!(
                "missing [package].version in {}",
                cargo_toml.display()
            ))
        })?;

    Version::parse(version_str).map_err(|_| {
        ReleaseError::Message(format!(
            "invalid semver {version_str} in {}",
            cargo_toml.display()
        ))
    })
}

/// Write `new_version` into `[package].version` in `Cargo.toml`, preserving
/// all comments and formatting.
pub(crate) fn write_version(cargo_toml: &Path, new_version: &Version) -> Result<()> {
    let raw = fs::read_to_string(cargo_toml)
        .map_err(|_| ReleaseError::Message(format!("reading {}", cargo_toml.display())))?;

    let mut doc: DocumentMut = raw
        .parse()
        .map_err(|_| ReleaseError::Message(format!("parsing {}", cargo_toml.display())))?;

    // Validate the key exists before mutating.
    if doc["package"]["version"].is_none() {
        return Err(ReleaseError::Message(format!(
            "[package].version not found in {}",
            cargo_toml.display()
        )));
    }

    doc["package"]["version"] = toml_edit::value(new_version.to_string());

    fs::write(cargo_toml, doc.to_string())
        .map_err(|_| ReleaseError::Message(format!("writing {}", cargo_toml.display())))?;

    Ok(())
}

/// Reads the package name from Cargo.toml
///
/// # Errors
/// Returns an error if:
/// - The file can't be read (I/O error)
/// - The TOML syntax is invalid
/// - The `[package].name` field is missing or not a string
pub fn read_name(cargo_toml: &Path) -> Result<String> {
    let raw = fs::read_to_string(cargo_toml)
        .map_err(|_| ReleaseError::Message(format!("reading {}", cargo_toml.display())))?;

    let doc: DocumentMut = raw
        .parse()
        .map_err(|_| ReleaseError::Message(format!("parsing {}", cargo_toml.display())))?;

    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            ReleaseError::Message(format!(
                "missing [package].name in {}",
                cargo_toml.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_cargo_toml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn read_version_basic() {
        let f = make_cargo_toml(
            r#"[package]
name = "my-crate"
version = "1.2.3"
edition = "2024"
"#,
        );
        let v = read_version(f.path()).unwrap();
        assert_eq!(v, Version::parse("1.2.3").unwrap());
    }

    #[test]
    fn write_version_preserves_formatting() {
        let original = r#"[package]
# Important crate
name = "my-crate"
version = "1.2.3"
edition = "2024"

[dependencies]
anyhow = "1"
"#;
        let f = make_cargo_toml(original);
        let new_v = Version::parse("1.3.0").unwrap();
        write_version(f.path(), &new_v).unwrap();

        let updated = fs::read_to_string(f.path()).unwrap();
        // Comment and other fields must survive.
        assert!(updated.contains("# Important crate"));
        assert!(updated.contains("anyhow = \"1\""));
        assert!(updated.contains("\"1.3.0\""));
        assert!(!updated.contains("\"1.2.3\""));
    }

    #[test]
    fn missing_version_errors() {
        let f = make_cargo_toml("[package]\nname = \"no-version\"\n");
        assert!(read_version(f.path()).is_err());
    }

    #[test]
    fn cargo_manifest_reads_version() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"[package]
name = "my-crate"
version = "1.2.3"
edition = "2024"
"#,
        )
        .unwrap();
        let manifest = CargoManifest;
        let v = manifest.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::parse("1.2.3").unwrap());
    }

    #[test]
    fn cargo_manifest_writes_version() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"[package]
name = "my-crate"
version = "1.2.3"
edition = "2024"
"#,
        )
        .unwrap();
        let manifest = CargoManifest;
        manifest
            .write_version(dir.path(), &Version::parse("1.4.0").unwrap())
            .unwrap();
        let updated = std::fs::read_to_string(&cargo_toml).unwrap();
        assert!(updated.contains("\"1.4.0\""));
        assert!(!updated.contains("\"1.2.3\""));
    }

    #[test]
    fn go_manifest_write_version_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = GoManifest;
        // should succeed without creating any files
        manifest
            .write_version(dir.path(), &Version::parse("1.2.0").unwrap())
            .unwrap();
        assert!(!dir.path().join("go.mod").exists());
    }

    #[test]
    fn go_manifest_implements_trait() {
        let manifest: &dyn ManifestBackend = &GoManifest;
        let _ = manifest;
    }

    #[test]
    fn npm_manifest_reads_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name": "my-pkg", "version": "1.2.3"}"#,
        )
        .unwrap();
        let manifest = NpmManifest;
        let v = manifest.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::parse("1.2.3").unwrap());
    }

    #[test]
    fn npm_manifest_writes_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name": "my-pkg", "version": "1.2.3"}"#,
        )
        .unwrap();
        let manifest = NpmManifest;
        manifest
            .write_version(dir.path(), &Version::parse("2.0.0").unwrap())
            .unwrap();
        let updated = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        assert!(updated.contains("\"2.0.0\""));
        assert!(!updated.contains("\"1.2.3\""));
    }

    #[test]
    fn go_manifest_read_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = GoManifest;
        let v = manifest.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(0, 0, 0));
    }

    #[test]
    fn go_manifest_write_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = GoManifest;
        manifest
            .write_version(dir.path(), &Version::parse("1.0.0").unwrap())
            .unwrap();
        // no files created
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn npm_manifest_errors_on_missing_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = NpmManifest;
        assert!(manifest.read_version(dir.path()).is_err());
    }

    #[test]
    fn npm_manifest_errors_on_missing_version_field() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name": "no-version"}"#).unwrap();
        let manifest = NpmManifest;
        assert!(manifest.read_version(dir.path()).is_err());
    }
    #[test]
    fn read_name_basic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "my-crate"
version = "1.2.3"
edition = "2024"
"#,
        )
        .unwrap();
        assert_eq!(
            read_name(&dir.path().join("Cargo.toml")).unwrap(),
            "my-crate"
        );
    }

    #[test]
    fn read_name_errors_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        assert!(read_name(&dir.path().join("Cargo.toml")).is_err());
    }
}
