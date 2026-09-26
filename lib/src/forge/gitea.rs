//! Gitea-compatible REST backends (Gitea, Forgejo, Codeberg).
//!
//! Only compiled with the `gitea` feature (which `forgejo` enables),
//! since this is the only backend that needs `ureq`.

use serde_json::{Value, json};

use super::{ForgeBackend, PrRequest};
use crate::errors::{ReleaseError, Result};

// -- Payloads (pure, so they're testable without a network) --

fn release_payload(tag: &str) -> Value {
    json!({
        "tag_name": tag,
        "name": tag,
        "draft": false,
        "prerelease": false
    })
}

fn pr_payload(pr: &PrRequest<'_>) -> Value {
    json!({
        "title": pr.title(),
        "body": pr.body,
        "head": pr.head,
        "base": pr.base,
    })
}

// -- Shared client --

/// Borrows from the forge struct instead of cloning its fields.
struct Client<'a> {
    host: &'a str,
    token: &'a str,
    owner: &'a str,
    repo: &'a str,
    name: &'static str,
}

impl Client<'_> {
    fn post(&self, endpoint: &str, payload: Value) -> Result<String> {
        let url = format!(
            "{}/api/v1/repos/{}/{}/{}",
            self.host.trim_end_matches('/'),
            self.owner,
            self.repo,
            endpoint
        );
        let response = ureq::post(&url)
            .header("Authorization", &format!("token {}", self.token))
            .header("Content-Type", "application/json")
            .send_json(payload)
            .map_err(|e| ReleaseError::Message(format!("{} POST {url}: {e}", self.name)))?;
        if !response.status().is_success() {
            return Err(ReleaseError::Message(format!(
                "{} API error: {}",
                self.name,
                response.status()
            )));
        }
        Ok(response.into_body().read_to_string()?)
    }

    fn create_release(&self, tag: &str) -> Result<()> {
        self.post("releases", release_payload(tag))?;
        Ok(())
    }

    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()> {
        let body = self.post("pulls", pr_payload(pr))?;
        let pr_url = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| v["html_url"].as_str().map(ToOwned::to_owned));
        if let Some(url) = pr_url {
            println!("  PR: {url}");
        }
        Ok(())
    }
}

// -- Public backends --

pub struct GiteaForge {
    pub host: String,
    pub token: String,
    pub owner: String,
    pub repo: String,
}

impl GiteaForge {
    fn client(&self) -> Client<'_> {
        Client {
            host: &self.host,
            token: &self.token,
            owner: &self.owner,
            repo: &self.repo,
            name: "gitea",
        }
    }
}

impl ForgeBackend for GiteaForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        self.client().create_release(tag)
    }

    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()> {
        self.client().create_pr(pr)
    }
}

pub struct ForgejoForge {
    pub host: String,
    pub token: String,
    pub owner: String,
    pub repo: String,
}

impl ForgejoForge {
    fn client(&self) -> Client<'_> {
        Client {
            host: &self.host,
            token: &self.token,
            owner: &self.owner,
            repo: &self.repo,
            name: "forgejo",
        }
    }
}

impl ForgeBackend for ForgejoForge {
    fn create_release(&self, tag: &str) -> Result<()> {
        self.client().create_release(tag)
    }

    fn create_pr(&self, pr: &PrRequest<'_>) -> Result<()> {
        self.client().create_pr(pr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_payload_maps_correctly() {
        let payload = release_payload("v1.2.3");
        assert_eq!(payload["tag_name"], "v1.2.3");
        assert_eq!(payload["name"], "v1.2.3");
        assert_eq!(payload["draft"], false);
        assert_eq!(payload["prerelease"], false);
    }

    #[test]
    fn pr_payload_maps_correctly() {
        let req = PrRequest {
            tag: "v1.2.3",
            head: "release/v1.2.3",
            base: "main",
            body: "Changelog here",
        };
        let payload = pr_payload(&req);
        assert_eq!(payload["title"], "chore: release v1.2.3");
        assert_eq!(payload["body"], "Changelog here");
        assert_eq!(payload["head"], "release/v1.2.3");
        assert_eq!(payload["base"], "main");
    }
}
