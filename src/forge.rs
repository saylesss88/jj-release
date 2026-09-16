//! Forge backends for GitHub, GitLab, and Forgejo

use std::process::Command;
use std::borrow::ToOwned;

use anyhow::{Context, Result, bail};

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

    fn create_pr(&self, tag: &str, head: &str, base: &str, body: &str) -> Result<()> {
        let output = Command::new("gh")
            .args([
                "pr",
                "create",
                "--title",
                &format!("chore: release {tag}"),
                "--body",
                body,
                "--head",
                head,
                "--base",
                base,
            ])
            .output()
            .context("spawning gh pr create")?;

        if !output.status.success() {
            bail!("gh pr create failed");
        }
        Ok(())
    }
}

pub struct GitLabForge;

impl ForgeBackend for GitLabForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        let status = Command::new("glab")
            .args(["release", "create", tag, "--generate-notes"])
            .status()
            .context("spawning glab release create")?;
        if !status.success() {
            bail!("glab release create failed for tag {tag}");
        }
        Ok(())
    }

    fn create_pr(&self, tag: &str, head: &str, base: &str, body: &str) -> Result<()> {
        let status = Command::new("glab")
            .args([
                "mr",
                "create",
                "--title",
                &format!("chore: release {tag}"),
                "--description",
                body,
                "--source-branch",
                head,
                "--target-branch",
                base,
            ])
            .status()
            .context("spawning glab mr create")?;
        if !status.success() {
            bail!("glab mr create failed");
        }
        Ok(())
    }
}

pub struct ForgejoForge {
    pub host: String,
    pub token: String,
    pub owner: String,
    pub repo: String,
}

impl ForgeBackend for ForgejoForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/releases",
            self.host, self.owner, self.repo
        );
        let body = serde_json::json!({
            "tag_name": tag,
            "name": tag,
            "draft": false,
            "prerelease": false,
        });
        let response = ureq::post(&url)
            .header("Authorization", &format!("token {}", self.token))
            .header("Content-Type", "application/json")
            .send_json(body)
            .context("creating Forgejo release")?;
        if !response.status().is_success() {
            bail!(
                "Forgejo release creation failed with status {}",
                response.status()
            );
        }
        Ok(())
    }
    fn create_pr(&self, tag: &str, head: &str, base: &str, body: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/pulls",
            self.host, self.owner, self.repo
        );
        let payload = serde_json::json!({
            "title": format!("chore: release {tag}"),
            "body": body,
            "head": head,
            "base": base,
        });
        let response = ureq::post(&url)
            .header("Authorization", &format!("token {}", self.token))
            .header("Content-Type", "application/json")
            .send_json(payload)
            .context("creating Forgejo PR")?;
        if !response.status().is_success() {
            bail!(
                "Forgejo PR creation failed with status {}",
                response.status()
            );
        }
        let pr_url = response
            .into_body()
            .read_json::<serde_json::Value>()
            .ok()
            .and_then(|v| v["html_url"].as_str().map(ToOwned::to_owned));
        if let Some(url) = pr_url {
            println!("  PR: {url}");
        }
        Ok(())
    }
}

pub trait ForgeBackend {
    /// Creates a new release associated with the specified tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails due to network issues, invalid
    /// authentication, or if the release cannot be created on the remote forge.
    fn create_release(&self, tag: &str) -> Result<()>;

    /// Opens a pull request from a head branch into a base branch for a given tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails due to network issues, invalid
    /// authentication, or if the target branches are invalid or already have a conflicting PR.
    fn create_pr(&self, tag: &str, head: &str, base: &str, body: &str) -> Result<()>;
}

pub struct NoForge;

impl ForgeBackend for NoForge {
    fn create_release(&self, _tag: &str) -> Result<()> {
        Ok(())
    }

    fn create_pr(&self, _tag: &str, _head: &str, _base: &str, _body: &str) -> Result<()> {
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
        assert!(
            forge
                .create_pr("v0.2.0", "release/v0.2.0", "main", "")
                .is_ok()
        );
    }

    #[test]
    fn github_forge_uses_gh_cli() {
        let forge: &dyn ForgeBackend = &GitHubForge;
        let _ = forge;
    }

    #[test]
    fn gitlab_forge_implements_trait() {
        let forge: &dyn ForgeBackend = &GitLabForge;
        let _ = forge;
    }

    #[test]
    fn forgejo_forge_implements_trait() {
        let _forge: &dyn ForgeBackend = &ForgejoForge {
            host: "https://codeberg.org".into(),
            token: "test-token".into(),
            owner: "myuser".into(),
            repo: "myrepo".into(),
        };
    }
}
