//! `Cargo.toml` reading and editing, preserving formatting via `toml_edit`.

use std::{fs, path::Path};

use semver::Version;
use toml_edit::{DocumentMut, Item, Table, Value};

use super::ManifestBackend;
use crate::errors::{ReleaseError, Result};

pub struct CargoManifest;

impl ManifestBackend for CargoManifest {
    fn read_version(&self, root: &Path) -> Result<Version> {
        read_version(&root.join("Cargo.toml"))
    }

    fn write_version(&self, root: &Path, version: &Version) -> Result<()> {
        write_version(&root.join("Cargo.toml"), version)
    }
}

/// Read and parse a TOML file into an editable document.
pub fn load_doc(path: &Path) -> Result<DocumentMut> {
    let raw = fs::read_to_string(path)
        .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;
    raw.parse()
        .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", path.display())))
}

/// Read the current `[package].version` from `Cargo.toml`.
pub fn read_version(cargo_toml: &Path) -> Result<Version> {
    let doc = load_doc(cargo_toml)?;

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

    Version::parse(version_str).map_err(|e| {
        ReleaseError::Message(format!(
            "invalid semver {version_str:?} in {}: {e}",
            cargo_toml.display()
        ))
    })
}

/// Write `new_version` into `[package].version` in `Cargo.toml`, preserving
/// all comments and formatting.
pub fn write_version(cargo_toml: &Path, new_version: &Version) -> Result<()> {
    let mut doc = load_doc(cargo_toml)?;

    // Validate the key exists before mutating.
    if doc["package"]["version"].is_none() {
        return Err(ReleaseError::Message(format!(
            "[package].version not found in {}",
            cargo_toml.display()
        )));
    }

    doc["package"]["version"] = toml_edit::value(new_version.to_string());

    fs::write(cargo_toml, doc.to_string())
        .map_err(|e| ReleaseError::Message(format!("writing {}: {e}", cargo_toml.display())))?;

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
    let doc = load_doc(cargo_toml)?;

    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            ReleaseError::Message(format!(
                "missing [package].name in {}",
                cargo_toml.display()
            ))
        })
}

/// Updates the version string for a specific dependency across all dependency tables.
pub fn bump_workspace_dependency(doc: &mut DocumentMut, crate_name: &str, new_version: &str) {
    let tables = ["dependencies", "dev-dependencies", "build-dependencies"];

    // Check standard crate-level dependency tables.
    for table_name in tables {
        if let Some(Item::Table(table)) = doc.get_mut(table_name) {
            update_dep_in_table(table, crate_name, new_version);
        }
    }

    // Check [workspace.dependencies].
    if let Some(Item::Table(workspace)) = doc.get_mut("workspace")
        && let Some(Item::Table(workspace_deps)) = workspace.get_mut("dependencies")
    {
        update_dep_in_table(workspace_deps, crate_name, new_version);
    }
    // Note: target-specific dependencies (target.'cfg(...)'.dependencies) not handled.
}

fn update_dep_in_table(table: &mut Table, crate_name: &str, new_version: &str) {
    if let Some(dep) = table.get_mut(crate_name) {
        match dep {
            // Shape 1: my_crate = "0.1.0"
            Item::Value(Value::String(_)) => {
                *dep = toml_edit::value(new_version);
            }
            // Shape 2: my_crate = { path = "../lib", version = "0.1.0" }
            Item::Value(Value::InlineTable(inline_table)) => {
                if inline_table.contains_key("version") {
                    inline_table.insert("version", new_version.into());
                }
            }
            // Shape 3: [dependencies.my_crate] \n version = "0.1.0"
            Item::Table(dep_table) if dep_table.contains_key("version") => {
                dep_table.insert("version", toml_edit::value(new_version));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    const BASIC: &str = r#"[package]
name = "my-crate"
version = "1.2.3"
edition = "2024"
"#;

    fn make_cargo_toml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn read_version_basic() {
        let f = make_cargo_toml(BASIC);
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
        write_version(f.path(), &Version::parse("1.3.0").unwrap()).unwrap();

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
        fs::write(dir.path().join("Cargo.toml"), BASIC).unwrap();
        let v = CargoManifest.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::parse("1.2.3").unwrap());
    }

    #[test]
    fn cargo_manifest_writes_version() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(&cargo_toml, BASIC).unwrap();
        CargoManifest
            .write_version(dir.path(), &Version::parse("1.4.0").unwrap())
            .unwrap();
        let updated = fs::read_to_string(&cargo_toml).unwrap();
        assert!(updated.contains("\"1.4.0\""));
        assert!(!updated.contains("\"1.2.3\""));
    }

    #[test]
    fn read_name_basic() {
        let f = make_cargo_toml(BASIC);
        assert_eq!(read_name(f.path()).unwrap(), "my-crate");
    }

    #[test]
    fn read_name_errors_when_missing() {
        let f = make_cargo_toml("[package]\nversion = \"1.0.0\"\n");
        assert!(read_name(f.path()).is_err());
    }

    #[test]
    fn bumps_inline_table_dependency() {
        let toml = r#"
[dependencies]
jj_release_core = { path = "../lib", version = "0.5.2" }
"#;
        let mut doc: DocumentMut = toml.parse().unwrap();
        bump_workspace_dependency(&mut doc, "jj_release_core", "0.6.0");
        assert!(doc.to_string().contains("\"0.6.0\""));
        assert!(!doc.to_string().contains("\"0.5.2\""));
    }
}
