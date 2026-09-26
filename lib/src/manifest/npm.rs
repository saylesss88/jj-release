//! `package.json` backend.
//!
//! Requires `serde_json`'s `preserve_order` feature so rewriting the file
//! keeps the user's key order instead of sorting keys alphabetically.

use std::{fs, path::Path};

use semver::Version;
use serde_json::Value;

use super::ManifestBackend;
use crate::errors::{ReleaseError, Result};

pub struct NpmManifest;

fn load_json(path: &Path) -> Result<(String, Value)> {
    let raw = fs::read_to_string(path)
        .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;
    let json = serde_json::from_str(&raw)
        .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", path.display())))?;
    Ok((raw, json))
}

impl ManifestBackend for NpmManifest {
    fn read_version(&self, root: &Path) -> Result<Version> {
        let (_, json) = load_json(&root.join("package.json"))?;
        let version_str = json["version"]
            .as_str()
            .ok_or_else(|| ReleaseError::Message("missing version field in package.json".into()))?;
        Version::parse(version_str).map_err(|e| {
            ReleaseError::Message(format!(
                "invalid semver {version_str:?} in package.json: {e}"
            ))
        })
    }
    fn write_version(&self, root: &Path, version: &Version) -> Result<()> {
        let path = root.join("package.json");
        let (raw, mut json) = load_json(&path)?;

        json["version"] = Value::String(version.to_string());

        let mut out = serde_json::to_string_pretty(&json)?;

        // npm writes a trailing newline; keep whatever the file had.
        if raw.ends_with('\n') {
            out.push('\n');
        }

        fs::write(&path, out)
            .map_err(|e| ReleaseError::Message(format!("writing {}: {e}", path.display())))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name": "my-pkg", "version": "1.2.3"}"#,
        )
        .unwrap();
        let v = NpmManifest.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::parse("1.2.3").unwrap());
    }

    #[test]
    fn writes_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name": "my-pkg", "version": "1.2.3"}"#,
        )
        .unwrap();
        NpmManifest
            .write_version(dir.path(), &Version::parse("2.0.0").unwrap())
            .unwrap();
        let updated = fs::read_to_string(dir.path().join("package.json")).unwrap();
        assert!(updated.contains("\"2.0.0\""));
        assert!(!updated.contains("\"1.2.3\""));
    }

    #[test]
    fn write_preserves_key_order_and_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(
            &path,
            "{\n  \"name\": \"my-pkg\",\n  \"version\": \"1.2.3\",\n  \"dependencies\": {}\n}\n",
        )
        .unwrap();
        NpmManifest
            .write_version(dir.path(), &Version::parse("1.3.0").unwrap())
            .unwrap();

        let updated = fs::read_to_string(&path).unwrap();
        let name = updated.find("\"name\"").unwrap();
        let version = updated.find("\"version\"").unwrap();
        let deps = updated.find("\"dependencies\"").unwrap();
        // Alphabetical would put "dependencies" first.
        assert!(
            name < version && version < deps,
            "key order changed:\n{updated}"
        );
        assert!(updated.ends_with("}\n"));
    }

    #[test]
    fn errors_on_missing_package_json() {
        let dir = tempfile::tempdir().unwrap();
        assert!(NpmManifest.read_version(dir.path()).is_err());
    }

    #[test]
    fn errors_on_missing_version_field() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name": "no-version"}"#).unwrap();
        assert!(NpmManifest.read_version(dir.path()).is_err());
    }
}
