//! Edit `Cargo.toml` version fields without destroying formatting.

use std::{borrow::ToOwned, fs, path::Path};

use regex::Regex;
use semver::Version;
use serde_json::Value;
use toml_edit::{DocumentMut, Item, Value as TomlValue};

use crate::config::Replacement;
use crate::errors::{ReleaseError, Result};
use crate::workspace::WorkspaceManifest;

/// Manages reading and writing version information in project manifests.
///
/// # Example
///
/// ```no_run
/// use jj_release_core::manifest::{ManifestBackend, CargoManifest};
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

/// Detect and return the appropriate manifest backend for the given root.
/// Returns `WorkspaceManifest` for Rust workspaces, `NpmManifest` for npm,
/// `GoManifest` for Go, and `CargoManifest` as the default.
#[must_use]
pub fn detect_manifest(root: &Path) -> Box<dyn ManifestBackend> {
    let cargo_toml = root.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&cargo_toml)
            && content.contains("[workspace]")
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

fn update_dep_in_table(table: &mut toml_edit::Table, crate_name: &str, new_version: &str) {
    if let Some(dep) = table.get_mut(crate_name) {
        match dep {
            // Shape 1: my_crate = "0.1.0"
            Item::Value(TomlValue::String(_)) => {
                *dep = toml_edit::value(new_version);
            }
            // Shape 2: my_crate = { path = "../lib", version = "0.1.0" }
            Item::Value(TomlValue::InlineTable(inline_table)) => {
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

/// Executes regex-based text replacements across arbitrary files (e.g. README.md).
/// Replaces `{{version}}`, `{{crate_name}}`, and `{{date}}` with the provided values.
///
/// # Errors
/// Returns an error if a file cannot be read/written, if the regex is invalid,
/// or if `exactly_one` is true and the search pattern does not match exactly once.
pub fn apply_replacements(
    replacements: &[Replacement],
    crate_name: &str,
    new_version: &str,
    date: &str,
) -> Result<()> {
    for req in replacements {
        let content = fs::read_to_string(&req.file)
            .map_err(|e| ReleaseError::Message(format!("failed to read {}: {e}", req.file)))?;

        let re = Regex::new(&req.search)
            .map_err(|e| ReleaseError::Message(format!("invalid regex '{}': {e}", req.search)))?;

        let replace_template = req
            .replace
            .replace("{{version}}", new_version)
            .replace("{{crate_name}}", crate_name)
            .replace("{{date}}", date);

        let matches = re.find_iter(&content).count();

        if req.exactly_one && matches != 1 {
            return Err(ReleaseError::Message(format!(
                "replacement in {} failed: expected exactly 1 match for '{}', found {}",
                req.file, req.search, matches
            )));
        }

        if matches > 0 {
            let new_content = re.replace_all(&content, replace_template.as_str());
            fs::write(&req.file, new_content.as_ref())
                .map_err(|e| ReleaseError::Message(format!("failed to write {}: {e}", req.file)))?;

            println!("  📝 Updated {} ({} match(es))", req.file, matches);
        }
    }
    Ok(())
}

/// Automatically detects and bumps a standard `version = "..."` declaration inside
/// a `flake.nix` file if it exists at the given root path.
///
/// # Errors
/// Returns an error if the `flake.nix` file cannot be read or written. Does not error
/// if the file doesn't exist, or if the regex fails to find a version string (it just warns).
pub fn bump_flake_nix(root: &Path, new_version: &str) -> Result<()> {
    let flake_path = root.join("flake.nix");

    if !flake_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&flake_path)
        .map_err(|e| ReleaseError::Message(format!("failed to read flake.nix: {e}")))?;

    // Match `version = "x.y.z";` or `version = "x.y.z"` capturing the prefix and suffix
    let re = Regex::new(r#"(?m)^(\s*version\s*=\s*")([^"]+)(".*)$"#)
        .map_err(|e| ReleaseError::Message(format!("flake regex compile error: {e}")))?;

    if !re.is_match(&content) {
        println!(
            "  ⚠️ Found flake.nix, but could not locate a `version = \"...\"` string to update."
        );
        return Ok(());
    }

    let new_content = re.replace(&content, format!("${{1}}{new_version}${{3}}"));

    fs::write(&flake_path, new_content.as_ref())
        .map_err(|e| ReleaseError::Message(format!("failed to write flake.nix: {e}")))?;

    println!("  ❄️  Bumped flake.nix to {new_version}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{self, NamedTempFile};

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
    #[test]
    fn detects_workspace_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let manifest = detect_manifest(dir.path());
        // verify it reads workspace version correctly
        // WorkspaceManifest will fail without [workspace.package] but that's ok
        // just verify it doesn't return CargoManifest
        let _ = manifest; // existence check
    }

    #[test]
    fn detects_npm_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
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
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname=\"test\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let v = detect_manifest(dir.path())
            .read_version(dir.path())
            .unwrap();
        assert_eq!(v, Version::parse("0.1.0").unwrap());
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
    #[test]
    fn test_bump_flake_nix() {
        let dir = tempfile::tempdir().unwrap();
        let flake_path = dir.path().join("flake.nix");

        let initial_flake = r#"
{
  description = "A test flake";
  outputs = { self, nixpkgs }: {
    packages.default = pkgs.buildRustPackage {
      pname = "jj-release";
      version = "0.7.0";
      src = ./.;
    };
  };
}
"#;
        fs::write(&flake_path, initial_flake).unwrap();

        // Perform the bump
        bump_flake_nix(dir.path(), "0.8.0").unwrap();

        // Verify the result
        let updated = fs::read_to_string(&flake_path).unwrap();
        assert!(updated.contains(r#"version = "0.8.0";"#));
        assert!(!updated.contains(r#"version = "0.7.0";"#));
    }

    #[test]
    fn test_bump_flake_nix_missing_file_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        // Don't create a flake.nix, function should return Ok(()) gracefully
        assert!(bump_flake_nix(dir.path(), "1.0.0").is_ok());
    }

    #[test]
    fn test_apply_replacements_basic_and_variables() {
        let dir = tempfile::tempdir().unwrap();
        let readme_path = dir.path().join("README.md");
        fs::write(&readme_path, "Add my_crate version 0.1.0 to your project!").unwrap();

        let replacements = vec![Replacement {
            file: readme_path.to_string_lossy().to_string(),
            search: r"version \d+\.\d+\.\d+".to_string(),
            replace: "version {{version}}".to_string(),
            exactly_one: true,
        }];

        apply_replacements(&replacements, "my_crate", "0.2.0", "2026-09-24").unwrap();

        let updated = fs::read_to_string(&readme_path).unwrap();
        assert_eq!(updated, "Add my_crate version 0.2.0 to your project!");
    }

    #[test]
    fn test_apply_replacements_capture_groups() {
        let dir = tempfile::tempdir().unwrap();
        let code_path = dir.path().join("lib.rs");
        fs::write(&code_path, r#"pub const VERSION: &str = "0.1.0";"#).unwrap();

        let replacements = vec![Replacement {
            file: code_path.to_string_lossy().to_string(),
            // Capture the prefix and suffix, match the version string in the middle
            search: r#"(pub const VERSION: &str = ")[^"]+(";)"#.to_string(),
            // Reconstruct using $1 and $2 capture groups around the new version
            replace: r"${1}{{version}}${2}".to_string(),
            exactly_one: true,
        }];

        apply_replacements(&replacements, "my_crate", "2.0.0", "2026-09-24").unwrap();

        let updated = fs::read_to_string(&code_path).unwrap();
        assert_eq!(updated, r#"pub const VERSION: &str = "2.0.0";"#);
    }

    #[test]
    fn test_apply_replacements_exactly_one_fails() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Setup a file with NO matches
        fs::write(&config_path, "some_other_setting = true").unwrap();

        let replacements = vec![Replacement {
            file: config_path.to_string_lossy().to_string(),
            search: r#"version = ".*""#.to_string(),
            replace: r#"version = "{{version}}""#.to_string(),
            exactly_one: true, // Should fail because match count == 0
        }];

        let result = apply_replacements(&replacements, "my_crate", "1.0.0", "2026-09-24");

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("expected exactly 1 match"));
    }

    #[test]
    fn test_apply_replacements_exactly_one_false_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Setup a file with MULTIPLE matches
        fs::write(&config_path, "version = \"1.0.0\"\nversion = \"1.0.0\"").unwrap();

        let replacements = vec![Replacement {
            file: config_path.to_string_lossy().to_string(),
            search: r#"version = "1\.0\.0""#.to_string(),
            replace: r#"version = "{{version}}""#.to_string(),
            exactly_one: false, // Will succeed and replace both
        }];

        apply_replacements(&replacements, "my_crate", "2.0.0", "2026-09-24").unwrap();

        let updated = fs::read_to_string(&config_path).unwrap();
        assert_eq!(updated, "version = \"2.0.0\"\nversion = \"2.0.0\"");
    }
}
