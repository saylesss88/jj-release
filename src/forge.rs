use std::process::Command;

use anyhow::{bail, Context, Result};

pub struct GitHubForge;

impl ForgeBackend for GitHubForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        let status = Command::new("gh")
            .args(["release", "create", tag, "--generate-notes"])
            .status()
            .context("spawning gh release create")?;
        if !status.success() {
            bail!("gh release create failed for tag {tag}");
        }
        Ok(())
    }

    fn create_pr(&self, tag: &str, head: &str, base: &str) -> Result<()> {
        let status = Command::new("gh")
            .args([
                "pr",
                "create",
                "--title",
                &format!("chore: release {tag}"),
                "--body",
                &format!("Automated release PR for {tag}"),
                "--head",
                head,
                "--base",
                base,
            ])
            .status()
            .context("spawning gh pr create")?;
        if !status.success() {
            bail!("gh pr create failed");
        }
        Ok(())
    }
}

pub trait ForgeBackend {
    fn create_release(&self, tag: &str) -> Result<()>;
    fn create_pr(&self, tag: &str, head: &str, base: &str) -> Result<()>;
}

pub struct NoForge;

impl ForgeBackend for NoForge {
    fn create_release(&self, _tag: &str) -> Result<()> {
        Ok(())
    }

    fn create_pr(&self, _tag: &str, _head: &str, _base: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_forge_create_release_is_noop() {
        let forge = NoForge;
        // should succeed without doing anything
        assert!(forge.create_release("v0.2.0").is_ok());
    }

    #[test]
    fn no_forge_create_pr_is_noop() {
        let forge = NoForge;
        assert!(forge.create_pr("v0.2.0", "release/v0.2.0", "main").is_ok());
    }

    #[test]
    fn github_forge_uses_gh_cli() {
        let _forge: &dyn ForgeBackend = &GitHubForge;
    }
}
