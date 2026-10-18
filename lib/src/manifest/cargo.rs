//! `Cargo.toml` reading and editing, preserving formatting via `toml_edit`.

use std::{fs, path::Path, process::Command};

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

    fn sync_lockfile(&self, root: &Path) -> Result<()> {
        sync_lockfile(root)
    }
}

/// Refreshes `Cargo.lock` after the workspace's own versions changed, without
/// touching any third-party dependency version.
///
/// `cargo update --workspace` only re-locks workspace members. It is tried with
/// `--offline` first, since nothing here needs the network when the registry
/// cache is warm, and retried online if that fails (for example on a cold
/// cache). Does nothing if there is no `Cargo.lock`.
///
/// # Errors
///
/// Returns an error if `cargo` cannot be spawned or `cargo update` fails.
pub fn sync_lockfile(root: &Path) -> Result<()> {
    if !root.join("Cargo.lock").exists() {
        return Ok(());
    }

    let update = |offline: bool| {
        let mut cmd = Command::new("cargo");
        cmd.args(["update", "--workspace"]);
        if offline {
            cmd.arg("--offline");
        }
        cmd.current_dir(root)
            .output()
            .map_err(|e| ReleaseError::Message(format!("spawning cargo update --workspace: {e}")))
    };

    let mut output = update(true)?;
    if !output.status.success() {
        output = update(false)?;
    }
    if !output.status.success() {
        return Err(ReleaseError::Message(format!(
            "cargo update --workspace failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
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

    // --- sync_lockfile -----------------------------------------------------
    // These shell out to the real `cargo`. The throwaway crates have no
    // dependencies, so everything runs `--offline`.

    fn cargo(dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("cargo")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
    }

    /// A one-crate project with a freshly generated `Cargo.lock`.
    fn crate_with_lock(dir: &Path, version: &str) {
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"locked-crate\"\nversion = \"{version}\"\nedition = \"2024\"\n"
            ),
        )
        .unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "").unwrap();
        assert!(
            cargo(dir, &["generate-lockfile", "--offline"])
                .status
                .success()
        );
    }

    /// `cargo metadata --locked` fails if `Cargo.lock` would need to change.
    fn lock_is_in_sync(dir: &Path) -> bool {
        cargo(
            dir,
            &["metadata", "--locked", "--offline", "--format-version", "1"],
        )
        .status
        .success()
    }

    #[test]
    fn version_bump_makes_lock_stale_and_sync_fixes_it() {
        let dir = tempfile::tempdir().unwrap();
        crate_with_lock(dir.path(), "1.0.0");
        assert!(lock_is_in_sync(dir.path()));

        CargoManifest
            .write_version(dir.path(), &Version::parse("1.1.0").unwrap())
            .unwrap();
        // Precondition: this is the state that used to get committed and
        // tagged, with `cargo publish` rewriting the lock file afterwards.
        assert!(
            !lock_is_in_sync(dir.path()),
            "a version bump should leave the lock stale"
        );

        CargoManifest.sync_lockfile(dir.path()).unwrap();
        assert!(lock_is_in_sync(dir.path()));
        let lock = fs::read_to_string(dir.path().join("Cargo.lock")).unwrap();
        assert!(lock.contains("version = \"1.1.0\""), "{lock}");
        assert!(!lock.contains("version = \"1.0.0\""), "{lock}");
    }

    #[test]
    fn sync_lockfile_leaves_a_synced_lock_untouched() {
        let dir = tempfile::tempdir().unwrap();
        crate_with_lock(dir.path(), "1.0.0");
        let before = fs::read(dir.path().join("Cargo.lock")).unwrap();

        CargoManifest.sync_lockfile(dir.path()).unwrap();

        assert_eq!(fs::read(dir.path().join("Cargo.lock")).unwrap(), before);
    }

    #[test]
    fn sync_lockfile_without_a_lockfile_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), BASIC).unwrap();

        CargoManifest.sync_lockfile(dir.path()).unwrap();

        // Must not create a lock file the project doesn't track.
        assert!(!dir.path().join("Cargo.lock").exists());
    }

    #[test]
    fn sync_lockfile_reports_cargo_failures() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "this is not toml [").unwrap();
        fs::write(dir.path().join("Cargo.lock"), "").unwrap();

        let err = CargoManifest.sync_lockfile(dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("cargo update --workspace failed"),
            "{err}"
        );
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
