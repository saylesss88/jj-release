//! Forge backends for GitHub, GitLab, and Forgejo

use std::{borrow::ToOwned, process::Command};

use crate::errors::{ReleaseError, Result};
use serde_json::Value;

pub struct GitHubForge;

impl ForgeBackend for GitHubForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        let status = Command::new("gh")
            .args(["release", "create", tag, "--generate-notes"])
            .status()
            .map_err(|_| ReleaseError::Message("spawning gh release create".into()))?;
        if !status.success() {
            return Err(ReleaseError::Message(format!(
                "gh release create failed for tag {tag}"
            )));
        }
        Ok(())
    }

    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()> {
        let output = Command::new("gh")
            .args([
                "pr",
                "create",
                "--title",
                &format!("chore: release {}", pr.tag),
                "--body",
                pr.body,
                "--head",
                pr.head,
                "--base",
                pr.base,
            ])
            .output()
            .map_err(|_| ReleaseError::Message("spawning gh pr create".into()))?;

        if !output.status.success() {
            return Err(ReleaseError::Message("gh pr create failed".to_string()));
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
            .map_err(|_| ReleaseError::Message("spawning glab release create".into()))?;
        if !status.success() {
            return Err(ReleaseError::Message(format!(
                "glab release create failed for tag {tag}"
            )));
        }
        Ok(())
    }

    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()> {
        let status = Command::new("glab")
            .args([
                "mr",
                "create",
                "--title",
                &format!("chore: release {}", pr.tag),
                "--description",
                pr.body,
                "--source-branch",
                pr.head,
                "--target-branch",
                pr.base,
            ])
            .status()
            .map_err(|_| ReleaseError::Message("spawning glab mr create".into()))?;
        if !status.success() {
            return Err(ReleaseError::Message("glab mr create failed".to_string()));
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

impl ForgejoForge {
    fn post(&self, endpoint: &str, payload: Value) -> Result<String> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/{}",
            self.host, self.owner, self.repo, endpoint
        );

        let response = ureq::post(&url)
            .header("Authorization", &format!("token {}", self.token))
            .header("Content-Type", "application/json")
            .send_json(payload)
            .map_err(|_| ReleaseError::Message(format!("POST {url}")))?;

        if !response.status().is_success() {
            return Err(ReleaseError::Message(format!(
                "forgejo API error: {}",
                response.status()
            )));
        }
        Ok(response.into_body().read_to_string()?)
    }
}

impl ForgeBackend for ForgejoForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        self.post(
            "releases",
            serde_json::json!({
                "tag_name": tag,
                "name": tag,
                "draft": false,
                "prerelease": false
            }),
        )?;
        Ok(())
    }
    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()> {
        let response_body = self.post(
            "pulls",
            serde_json::json!({
                "title": format!("chore: release {}", pr.tag),
                "body": pr.body,
                "head": pr.head,
                "base": pr.base,
            }),
        )?;
        let pr_url = serde_json::from_str::<Value>(&response_body)
            .ok()
            .and_then(|v| v["html_url"].as_str().map(ToOwned::to_owned));
        if let Some(url) = pr_url {
            println!("  PR: {url}");
        }
        Ok(())
    }
}

pub struct PrRequest<'a> {
    pub tag: &'a str,
    pub head: &'a str,
    pub base: &'a str,
    pub body: &'a str,
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

#[must_use]
pub fn github_release_args(tag: &str) -> Vec<String> {
    vec![
        "release".to_owned(),
        "create".to_owned(),
        tag.to_owned(),
        "--generate-notes".to_owned(),
    ]
}

#[must_use]
pub fn github_pr_args(pr: &PrRequest<'_>) -> Vec<String> {
    vec![
        "pr".to_owned(),
        "create".to_owned(),
        "--title".to_owned(),
        format!("chore: release {}", pr.tag),
        "--body".to_owned(),
        pr.body.to_owned(),
        "--head".to_owned(),
        pr.head.to_owned(),
        "--base".to_owned(),
        pr.base.to_owned(),
    ]
}

#[must_use]
pub fn gitlab_release_args(tag: &str) -> Vec<String> {
    vec![
        "release".to_owned(),
        "create".to_owned(),
        tag.to_owned(),
        "--generate-notes".to_owned(),
    ]
}

#[must_use]
pub fn gitlab_pr_args(pr: &PrRequest<'_>) -> Vec<String> {
    vec![
        "mr".to_owned(),
        "create".to_owned(),
        "--title".to_owned(),
        format!("chore: release {}", pr.tag),
        "--description".to_owned(),
        pr.body.to_owned(),
        "--source-branch".to_owned(),
        pr.head.to_owned(),
        "--target-branch".to_owned(),
        pr.base.to_owned(),
    ]
}

#[must_use]
pub fn forgejo_release_payload(tag: &str) -> Value {
    serde_json::json!({
        "tag_name": tag,
        "name": tag,
        "draft": false,
        "prerelease": false
    })
}

#[must_use]
pub fn forgejo_pr_payload(pr: &PrRequest<'_>) -> Value {
    serde_json::json!({
        "title": format!("chore: release {}", pr.tag),
        "body": pr.body,
        "head": pr.head,
        "base": pr.base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_pr_args_maps_correctly() {
        let req = PrRequest {
            tag: "v1.2.3",
            head: "release/v1.2.3",
            base: "main",
            body: "Changelog here",
        };
        let args = github_pr_args(&req);

        assert_eq!(
            args,
            vec![
                "pr",
                "create",
                "--title",
                "chore: release v1.2.3",
                "--body",
                "Changelog here",
                "--head",
                "release/v1.2.3",
                "--base",
                "main"
            ]
        );
    }

    #[test]
    fn gitlab_pr_args_maps_correctly() {
        let req = PrRequest {
            tag: "v1.2.3",
            head: "release/v1.2.3",
            base: "main",
            body: "Changelog here",
        };
        let args = gitlab_pr_args(&req);

        assert_eq!(
            args,
            vec![
                "mr",
                "create",
                "--title",
                "chore: release v1.2.3",
                "--description",
                "Changelog here",
                "--source-branch",
                "release/v1.2.3",
                "--target-branch",
                "main"
            ]
        );
    }

    #[test]
    fn forgejo_pr_payload_maps_correctly() {
        let req = PrRequest {
            tag: "v1.2.3",
            head: "release/v1.2.3",
            base: "main",
            body: "Changelog here",
        };
        let payload = forgejo_pr_payload(&req);

        assert_eq!(payload["title"], "chore: release v1.2.3");
        assert_eq!(payload["body"], "Changelog here");
        assert_eq!(payload["head"], "release/v1.2.3");
        assert_eq!(payload["base"], "main");
    }

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
                .create_pr(&PrRequest {
                    tag: "v0.2.0",
                    head: "release/v0.2.0",
                    base: "main",
                    body: "",
                })
                .is_ok()
        );
    }
}
