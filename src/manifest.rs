//! Edit `Cargo.toml` version fields without destroying formatting.

use std::path::Path;

use anyhow::{bail, Context, Result};
use semver::Version;
use toml_edit::DocumentMut;

pub trait ManifestBackend {
    fn read_version(&self, root: &Path) -> Result<Version>;
    fn write_version(&self, root: &Path, version: &Version) -> Result<()>;
}

pub struct CargoManifest;
pub struct GoManifest;

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

/// Read the current `[package].version` from `Cargo.toml`.
pub fn read_version(cargo_toml: &Path) -> Result<Version> {
    let raw = std::fs::read_to_string(cargo_toml)
        .with_context(|| format!("reading {}", cargo_toml.display()))?;

    let doc: DocumentMut = raw
        .parse()
        .with_context(|| format!("parsing {}", cargo_toml.display()))?;

    let version_str = doc
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .with_context(|| format!("missing [package].version in {}", cargo_toml.display()))?;

    Version::parse(version_str)
        .with_context(|| format!("invalid semver {version_str:?} in {}", cargo_toml.display()))
}

/// Write `new_version` into `[package].version` in `Cargo.toml`, preserving
/// all comments and formatting.
pub fn write_version(cargo_toml: &Path, new_version: &Version) -> Result<()> {
    let raw = std::fs::read_to_string(cargo_toml)
        .with_context(|| format!("reading {}", cargo_toml.display()))?;

    let mut doc: DocumentMut = raw
        .parse()
        .with_context(|| format!("parsing {}", cargo_toml.display()))?;

    // Validate the key exists before mutating.
    if doc["package"]["version"].is_none() {
        bail!("[package].version not found in {}", cargo_toml.display());
    }

    doc["package"]["version"] = toml_edit::value(new_version.to_string());

    std::fs::write(cargo_toml, doc.to_string())
        .with_context(|| format!("writing {}", cargo_toml.display()))?;

    Ok(())
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

        let updated = std::fs::read_to_string(f.path()).unwrap();
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
        std::fs::write(
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
        std::fs::write(
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
        let _manifest: &dyn ManifestBackend = &GoManifest;
    }
}
