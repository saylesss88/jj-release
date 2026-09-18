//! Cargo registry checks.

use anyhow::{Result, anyhow};
use semver::Version;
use ureq::{Agent, Error};

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
        Err(e) => Err(anyhow!("crates.io API error: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_version_exists() {
        // anyhow 1.0.0 definitely exists on crates.io
        let v = Version::parse("1.0.0").unwrap();
        assert!(version_exists_on_crates_io("anyhow", &v).unwrap());
    }

    #[test]
    fn unpublished_version_does_not_exist() {
        // This version should never exist
        let v = Version::parse("99.99.99").unwrap();
        assert!(!version_exists_on_crates_io("anyhow", &v).unwrap());
    }
}
