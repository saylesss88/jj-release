//! GitLab backend (shells out to `glab`).

use std::process::Command;

use super::{ForgeBackend, PrRequest};
use crate::errors::{ReleaseError, Result};

pub struct GitLabForge;

impl GitLabForge {
    fn release_args(tag: &str) -> Vec<String> {
        vec![
            "release".into(),
            "create".into(),
            tag.into(),
            "--generate-notes".into(),
        ]
    }

    fn mr_args(pr: &PrRequest<'_>) -> Vec<String> {
        vec![
            "mr".into(),
            "create".into(),
            "--title".into(),
            pr.title(),
            "--description".into(),
            pr.body.into(),
            "--source-branch".into(),
            pr.head.into(),
            "--target-branch".into(),
            pr.base.into(),
        ]
    }
}

impl ForgeBackend for GitLabForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        let status = Command::new("glab")
            .args(Self::release_args(tag))
            .status()
            .map_err(|e| ReleaseError::Message(format!("spawning glab release create: {e}")))?;
        if !status.success() {
            return Err(ReleaseError::Message(format!(
                "glab release create failed for tag {tag}"
            )));
        }
        Ok(())
    }

    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()> {
        let status = Command::new("glab")
            .args(Self::mr_args(pr))
            .status()
            .map_err(|e| ReleaseError::Message(format!("spawning glab mr create: {e}")))?;
        if !status.success() {
            return Err(ReleaseError::Message("glab mr create failed".to_string()));
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
            GitLabForge::release_args("v1.2.3"),
            vec!["release", "create", "v1.2.3", "--generate-notes"]
        );
    }

    #[test]
    fn mr_args_map_correctly() {
        let req = PrRequest {
            tag: "v1.2.3",
            head: "release/v1.2.3",
            base: "main",
            body: "Changelog here",
        };
        assert_eq!(
            GitLabForge::mr_args(&req),
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
                "main",
            ]
        );
    }
}
