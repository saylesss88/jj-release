//! Cargo registry checks.

use semver::Version;
use serde_json::Value;
use ureq::{Agent, Error};

use crate::errors::{ReleaseError, Result};

/// Check if a specific version of a crate is already published on crates.io
///
/// # Errors
/// Returns an error if the network request fails or if the API returns an unexpected
/// status code other than a success (2xx) or 404 Not Found.
pub fn version_exists_on_crates_io(name: &str, version: &Version) -> Result<bool> {
    let agent = Agent::new_with_defaults();
    let url = format!("https://crates.io/api/v1/crates/{name}/{version}");
    let response = agent
        .get(&url)
        .header(
            "User-Agent",
            "jj-release (github.com/saylesss88/jj-release)",
        )
        .call();
    match response {
        Ok(r) => Ok(r.status().is_success()),
        Err(Error::StatusCode(404)) => Ok(false),
        Err(e) => Err(ReleaseError::Message(format!("crates.io API error: {e}"))),
    }
}

/// Fetches the latest published version of a crate from the crates.io REST API.
///
/// # Errors
/// Returns an error if the network request fails, JSON deserialization fails, or the API returns
/// an unexpected non-404 status code.
pub fn latest_version_on_crates_io(name: &str) -> Result<Option<Version>> {
    let agent = Agent::new_with_defaults();
    let url = format!("https://crates.io/api/v1/crates/{name}");
    let response = agent
        .get(&url)
        .header(
            "User-Agent",
            "jj-release (github.com/saylesss88/jj-release)",
        )
        .call();
    match response {
        Ok(r) => {
            let json: Value = r.into_body().read_json()?;
            let version_str = json["crate"]["newest_version"].as_str();
            Ok(version_str.and_then(|v| Version::parse(v).ok()))
        }
        Err(Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(ReleaseError::Message(format!("crates.io API error: {e}"))),
    }
}

#[cfg(test)]
#[ignore = "requires network access to crates.io"]
mod tests {
    use super::*;

    #[test]
    fn published_version_exists() {
        // anyhow 1.0.0 definitely exists on crates.io
        let v = Version::parse("1.0.0").unwrap();
        assert!(version_exists_on_crates_io("thiserror", &v).unwrap());
    }

    #[test]
    fn unpublished_version_does_not_exist() {
        // This version should never exist
        let v = Version::parse("99.99.99").unwrap();
        assert!(!version_exists_on_crates_io("thiserror", &v).unwrap());
    }
    #[test]
    fn gets_latest_version_from_crates_io() {
        // thiserror is stable and will always have a version
        let v = latest_version_on_crates_io("thiserror").unwrap();
        assert!(v.is_some());
        assert!(v.unwrap().major >= 1);
    }

    #[test]
    fn returns_none_for_nonexistent_crate() {
        let v =
            latest_version_on_crates_io("this-crate-definitely-does-not-exist-xyzzy123").unwrap();
        assert!(v.is_none());
    }
}
