//! Publish backends for Cargo, npm, and no-op.

use std::{path::Path, process::Command};

use anyhow::{Context, Result, bail};

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
        let output = Command::new("cargo")
            .args(["publish", "--dry-run", "--allow-dirty"])
            .current_dir(root)
            .output()
            .context("spawning cargo publish --dry-run")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("cargo publish --dry-run failed:\n{stderr}");
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
            .context("spawning cargo publish")?;
        if !status.success() {
            bail!("cargo publish failed");
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
            .context("spawning npm publish --dry-run")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("npm publish --dry-run failed:\n{stderr}");
        }
        Ok(())
    }
    fn publish(&self, root: &Path, extra_flags: &[String]) -> Result<()> {
        let status = Command::new("npm")
            .arg("publish")
            .args(extra_flags)
            .current_dir(root)
            .status()
            .context("spawning npm publish")?;
        if !status.success() {
            bail!("npm publish failed");
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
    let output = Command::new("cargo")
        .args(["semver-checks"])
        .current_dir(root)
        .output()
        .context("spawning cargo semver-checks, is cargo-semver-checks installed?")?;
    // Exit code 0 = no breaking changes, non-zero = breaking changes detected
    Ok(!output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

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
