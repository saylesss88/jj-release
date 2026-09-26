//! Forge backends for GitHub, GitLab, Forgejo, and Gitea.
//!
//! Each backend lives behind a Cargo feature:
//! - `github`  -> [`GitHubForge`] (shells out to `gh`)
//! - `gitlab`  -> [`GitLabForge`] (shells out to `glab`)
//! - `gitea`   -> [`GiteaForge`] and [`ForgejoForge`] (REST via `ureq`)
//! - `forgejo` -> alias that enables `gitea`

#[cfg(feature = "gitea")]
mod gitea;
#[cfg(feature = "github")]
mod github;
#[cfg(feature = "gitlab")]
mod gitlab;

#[cfg(feature = "gitea")]
pub use gitea::{ForgejoForge, GiteaForge};
#[cfg(feature = "github")]
pub use github::GitHubForge;
#[cfg(feature = "gitlab")]
pub use gitlab::GitLabForge;

use crate::errors::Result;

// -- Types --

pub struct PrRequest<'a> {
    pub tag: &'a str,
    pub head: &'a str,
    pub base: &'a str,
    pub body: &'a str,
}

impl PrRequest<'_> {
    /// Title used for the release PR/MR on every forge.
    #[must_use]
    pub fn title(&self) -> String {
        format!("chore: release {}", self.tag)
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
    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()>;
}

pub struct NoForge;

impl ForgeBackend for NoForge {
    fn create_release(&self, _tag: &str) -> Result<()> {
        Ok(())
    }

    fn create_pr(&self, _pr: &PrRequest<'_>) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_title_uses_tag() {
        let req = PrRequest {
            tag: "v1.2.3",
            head: "release/v1.2.3",
            base: "main",
            body: "",
        };
        assert_eq!(req.title(), "chore: release v1.2.3");
    }

    #[test]
    fn no_forge_create_release_is_noop() {
        assert!(NoForge.create_release("v0.2.0").is_ok());
    }

    #[test]
    fn no_forge_create_pr_is_noop() {
        let req = PrRequest {
            tag: "v0.2.0",
            head: "release/v0.2.0",
            base: "main",
            body: "",
        };
        assert!(NoForge.create_pr(&req).is_ok());
    }
}
