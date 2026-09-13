//! Publish backends for Cargo, npm, and no-op.

use std::{path::Path, process::Command};

use anyhow::{Context, Result, bail};

pub trait PublishBackend {
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
    fn publish(&self, _root: &Path, _extra_flags: &[String]) -> Result<()> {
        Ok(())
    }
}

pub struct CargoPublish;

impl PublishBackend for CargoPublish {
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
}
