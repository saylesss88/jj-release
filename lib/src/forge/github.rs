//! GitHub backend (shells out to `gh`).

use std::process::Command;

use super::{ForgeBackend, PrRequest};
use crate::errors::{ReleaseError, Result};

pub struct GitHubForge;

impl GitHubForge {
    fn release_args(tag: &str) -> Vec<String> {
        vec![
            "release".into(),
            "create".into(),
            tag.into(),
            "--generate-notes".into(),
        ]
    }

    fn pr_args(pr: &PrRequest<'_>) -> Vec<String> {
        vec![
            "pr".into(),
            "create".into(),
            "--title".into(),
            pr.title(),
            "--body".into(),
            pr.body.into(),
            "--head".into(),
            pr.head.into(),
            "--base".into(),
            pr.base.into(),
        ]
    }
}

impl ForgeBackend for GitHubForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        let status = Command::new("gh")
            .args(Self::release_args(tag))
            .status()
            .map_err(|e| ReleaseError::Message(format!("spawning gh release create: {e}")))?;
        if !status.success() {
            return Err(ReleaseError::Message(format!(
                "gh release create failed for tag {tag}"
            )));
        }
        Ok(())
    }

    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()> {
        let output = Command::new("gh")
            .args(Self::pr_args(pr))
            .output()
            .map_err(|e| ReleaseError::Message(format!("spawning gh pr create: {e}")))?;

        if !output.status.success() {
            return Err(ReleaseError::Message(format!(
                "gh pr create failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        // `gh pr create` prints the new PR's URL on stdout.
        let url = String::from_utf8_lossy(&output.stdout);
        let url = url.trim();
        if !url.is_empty() {
            println!("  PR: {url}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_args_map_correctly() {
        assert_eq!(
            GitHubForge::release_args("v1.2.3"),
            vec!["release", "create", "v1.2.3", "--generate-notes"]
        );
    }

    #[test]
    fn pr_args_map_correctly() {
        let req = PrRequest {
            tag: "v1.2.3",
            head: "release/v1.2.3",
            base: "main",
            body: "Changelog here",
        };
        assert_eq!(
            GitHubForge::pr_args(&req),
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
                "main",
            ]
        );
    }
}
