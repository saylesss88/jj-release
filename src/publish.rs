use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

pub trait PublishBackend {
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
        assert!(publisher
            .publish(&std::path::Path::new("/tmp"), &[])
            .is_ok());
    }

    #[test]
    fn no_publish_implements_trait() {
        let _publisher: &dyn PublishBackend = &NoPublish;
    }
}
