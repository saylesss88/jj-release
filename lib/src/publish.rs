//! Publish backends for Cargo, npm, and no-op.

use std::{fs, path::Path, process::Command};

use cargo_metadata::MetadataCommand;

use crate::errors::{ReleaseError, Result};

pub trait PublishBackend {
    /// Runs pre-flight checks (like a dry-run) to ensure the package can be published.
    ///
    /// # Errors
    ///
    /// Returns an error if the dry-run fails (e.g., uncommitted files without --allow-dirty,
    /// missing metadata, or missing registry tokens).
    fn check(&self, root: &Path) -> Result<()>;
    /// Publishes the project crate to a package registry (such as crates.io).
    ///
    /// # Errors
    ///
    /// Returns an error if the publishing process fails due to network issues,
    /// compilation/packaging errors, or if the registry rejects the upload.
    fn publish(&self, root: &Path, extra_flags: &[String]) -> Result<()>;
}

pub struct NoPublish;

impl PublishBackend for NoPublish {
    fn check(&self, _root: &Path) -> Result<()> {
        Ok(())
    }
    fn publish(&self, _root: &Path, _extra_flags: &[String]) -> Result<()> {
        Ok(())
    }
}

pub struct CargoPublish;

impl PublishBackend for CargoPublish {
    fn check(&self, root: &Path) -> Result<()> {
        // crates.io rejects these on upload, after earlier crates may already
        // be published, so catch them before anything happens.
        check_publish_metadata(root)?;

        let output = Command::new("cargo")
            .args(["publish", "--dry-run", "--allow-dirty", "--no-verify"])
            .current_dir(root)
            .output()
            .map_err(|_| ReleaseError::Message("spawning cargo publish --dry-run".into()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ReleaseError::Message(format!(
                "cargo publish --dry-run failed:\n{stderr}"
            )));
        }
        Ok(())
    }

    fn publish(&self, root: &Path, extra_flags: &[String]) -> Result<()> {
        let status = Command::new("cargo")
            .arg("publish")
            .arg("--allow-dirty")
            .args(extra_flags)
            .current_dir(root)
            .status()
            .map_err(|_| ReleaseError::Message("spawning cargo publish".into()))?;
        if !status.success() {
            return Err(ReleaseError::Message("cargo publish failed".to_string()));
        }
        Ok(())
    }
}

pub struct NpmPublish;

impl PublishBackend for NpmPublish {
    fn check(&self, root: &Path) -> Result<()> {
        let output = Command::new("npm")
            .args(["publish", "--dry-run"])
            .current_dir(root)
            .output()
            .map_err(|_| ReleaseError::Message("spawning npm publish --dry-run".into()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ReleaseError::Message(format!(
                "npm publish --dry-run failed:\n{stderr}"
            )));
        }
        Ok(())
    }
    fn publish(&self, root: &Path, extra_flags: &[String]) -> Result<()> {
        let status = Command::new("npm")
            .arg("publish")
            .args(extra_flags)
            .current_dir(root)
            .status()
            .map_err(|_| ReleaseError::Message("spawning npm publish".into()))?;
        if !status.success() {
            return Err(ReleaseError::Message("npm publish failed".to_string()));
        }
        Ok(())
    }
}

/// Runs `cargo-semver-checks` on the crate located at the specified root directory.
///
/// # Returns
/// * `Ok(true)` if breaking API changes were detected (indicated by a non-zero exit code).
/// * `Ok(false)` if no breaking changes were found (indicated by a zero exit code).
///
/// # Errors
/// Returns an error if the process fails to spawn or if `cargo-semver-checks`
pub fn run_semver_checks(root: &Path) -> Result<bool> {
    let installed = Command::new("cargo")
        .args(["semver-checks", "--version"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !installed {
        return Err(ReleaseError::Message(
            "cargo-semver-checks is not installed (cargo install cargo-semver-checks)".into(),
        ));
    }

    let output = Command::new("cargo")
        .args(["semver-checks"])
        .current_dir(root)
        .output()
        .map_err(|e| ReleaseError::Message(format!("spawning cargo semver-checks: {e}")))?;
    // Exit code 0 = no breaking changes, non-zero = breaking changes detected
    Ok(!output.status.success())
}

/// crates.io rejects packages without a description or license. Check every
/// published member up front, so a release can't fail halfway through.
fn check_publish_metadata(dir: &Path) -> Result<()> {
    let dir = fs::canonicalize(dir)?;
    let dir = dir.as_path();

    let metadata = MetadataCommand::new()
        .manifest_path(dir.join("Cargo.toml"))
        .no_deps()
        .exec()?;

    let manifest = dir.join("Cargo.toml");
    let packages: Vec<_> = metadata
        .workspace_packages()
        .into_iter()
        .filter(|p| !matches!(&p.publish, Some(registries) if registries.is_empty()))
        .collect();
    // A member directory: just that package. The workspace root (virtual or
    // not): every published member.
    let in_dir: Vec<_> = packages
        .iter()
        .copied()
        .filter(|p| p.manifest_path.as_std_path() == manifest)
        .collect();
    let checked = if in_dir.is_empty() || metadata.workspace_root.as_std_path() == dir {
        packages
    } else {
        in_dir
    };

    let problems: Vec<String> = checked
        .iter()
        .filter_map(|p| {
            let mut missing = Vec::new();
            if p.description.as_deref().is_none_or(str::is_empty) {
                missing.push("description");
            }
            if p.license.is_none() && p.license_file.is_none() {
                missing.push("license or license-file");
            }
            (!missing.is_empty()).then(|| format!("{}: missing {}", p.name, missing.join(", ")))
        })
        .collect();

    if problems.is_empty() {
        Ok(())
    } else {
        Err(ReleaseError::Message(format!(
            "crates.io would reject these packages:\n  {}",
            problems.join("\n  ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    /// A member crate that `cargo metadata` accepts.
    fn write_member(root: &Path, dir: &str, manifest: &str) {
        let d = root.join(dir);
        fs::create_dir_all(d.join("src")).unwrap();
        fs::write(d.join("Cargo.toml"), manifest).unwrap();
        fs::write(d.join("src/lib.rs"), "").unwrap();
    }

    #[test]
    fn metadata_check_names_members_missing_license() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"core\", \"cli\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        write_member(
            dir.path(),
            "core",
            "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         description = \"core\"\nlicense = \"MIT\"\n",
        );
        write_member(
            dir.path(),
            "cli",
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         description = \"cli\"\n",
        );

        // Workspace root: every published member is checked, and only cli fails.
        let err = check_publish_metadata(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("cli: missing license or license-file"),
            "{err}"
        );
        assert!(!err.contains("core:"), "{err}");

        // Member directories are checked on their own.
        assert!(check_publish_metadata(&dir.path().join("core")).is_ok());
        assert!(check_publish_metadata(&dir.path().join("cli")).is_err());
    }

    #[test]
    fn metadata_check_accepts_inherited_license_and_skips_unpublished() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"cli\", \"xtask\"]\nresolver = \"2\"\n\n\
         [workspace.package]\nlicense = \"Apache-2.0\"\n",
        )
        .unwrap();
        // The jj-compose case: license inherited from the workspace.
        write_member(
            dir.path(),
            "cli",
            "[package]\nname = \"cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
         description = \"cli\"\nlicense.workspace = true\n",
        );
        // Missing everything, but never published, so it doesn't count.
        write_member(
            dir.path(),
            "xtask",
            "[package]\nname = \"xtask\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n",
        );

        assert!(check_publish_metadata(dir.path()).is_ok());
    }

    #[test]
    fn metadata_check_requires_description() {
        let dir = tempfile::tempdir().unwrap();
        write_member(
            dir.path(),
            ".",
            "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"MIT\"\n",
        );

        let err = check_publish_metadata(dir.path()).unwrap_err().to_string();
        assert!(err.contains("solo: missing description"), "{err}");
    }

    #[test]
    fn no_publish_is_noop() {
        let publisher = NoPublish;
        assert!(publisher.publish(Path::new("/tmp"), &[]).is_ok());
    }

    #[test]
    fn no_publish_implements_trait() {
        let publisher: &dyn PublishBackend = &NoPublish;
        let _ = publisher;
    }

    #[test]
    fn no_publish_ignores_flags() {
        let publisher = NoPublish;
        let flags = vec!["--dry-run".to_owned(), "--locked".to_owned()];
        assert!(publisher.publish(Path::new("/tmp"), &flags).is_ok());
    }

    #[test]
    fn semver_checks_handles_missing_tool() {
        // If cargo-semver-checks isn't installed this should error gracefully
        // rather than panic.
        let dir = tempfile::tempdir().unwrap();
        let _ = run_semver_checks(dir.path());
    }
    #[test]
    fn no_publish_check_is_noop() {
        let publisher = NoPublish;
        assert!(publisher.check(Path::new("/tmp")).is_ok());
    }
}
