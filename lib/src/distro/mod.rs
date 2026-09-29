//! Publishing new versions to distribution package repositories.
//!
//! Each target is behind its own Cargo feature:
//! - `publish-aur`  -> [`aur`]
//! - `publish-copr` -> [`copr`]

#[cfg(feature = "publish-aur")]
pub mod aur;
#[cfg(feature = "publish-copr")]
pub mod copr;

use std::path::Path;

use semver::Version;

use crate::{config::Config, errors::Result};

/// A named pre-flight check: `Ok(detail)` or `Err(reason)`.
pub type Check = (&'static str, std::result::Result<String, String>);

/// Whether any distro target is configured (and compiled in).
#[must_use]
#[allow(unused_mut, unused_variables)] // only used by some feature combinations
pub const fn any_configured(config: &Config) -> bool {
    let mut any = false;
    #[cfg(feature = "publish-aur")]
    {
        any |= config.publish.aur.is_some();
    }
    #[cfg(feature = "publish-copr")]
    {
        any |= config.publish.copr.is_some();
    }
    any
}

/// Pre-flight checks for every configured target. Makes a few quick network
/// calls (AUR access, COPR auth) but changes nothing.
#[must_use]
#[allow(unused_mut, unused_variables)]
pub fn preflight(config: &Config, root: &Path) -> Vec<Check> {
    let mut checks = Vec::new();
    #[cfg(feature = "publish-aur")]
    if let Some(aur) = &config.publish.aur {
        checks.extend(aur::preflight(aur));
    }
    #[cfg(feature = "publish-copr")]
    if let Some(copr) = &config.publish.copr {
        checks.extend(copr::preflight(copr, root));
    }
    checks
}

/// One line per configured target describing what a release would do.
#[must_use]
#[allow(unused_mut, unused_variables)]
pub fn describe(config: &Config, version: &Version) -> Vec<String> {
    let mut lines = Vec::new();
    #[cfg(feature = "publish-aur")]
    if let Some(aur) = &config.publish.aur {
        lines.push(format!(
            "AUR: {} → {version} (pkgrel 1, checksums from the crates.io release)",
            aur.package
        ));
    }
    #[cfg(feature = "publish-copr")]
    if let Some(copr) = &config.publish.copr {
        lines.push(format!(
            "COPR: {} ← SRPM from {} at {version}{}",
            copr.project,
            copr.spec.display(),
            if copr.wait {
                " (waits for the build)"
            } else {
                ""
            }
        ));
    }
    lines
}

/// Update every configured target. Returns each target's result so the
/// caller decides whether failures are fatal.
#[must_use]
#[allow(unused_mut, unused_variables)]
pub fn update_all(
    config: &Config,
    root: &Path,
    version: &Version,
) -> Vec<(&'static str, Result<()>)> {
    let mut results = Vec::new();
    #[cfg(feature = "publish-aur")]
    if let Some(aur) = &config.publish.aur {
        results.push(("AUR", aur::publish(aur, version, false)));
    }
    #[cfg(feature = "publish-copr")]
    if let Some(copr) = &config.publish.copr {
        results.push(("COPR", copr::publish(copr, root, version, false)));
    }
    results
}

// -- Shared helpers for the target modules --

/// `Ok("a, b, c")` if every tool is on PATH, otherwise the missing ones.
#[cfg(any(feature = "publish-aur", feature = "publish-copr"))]
fn require_tools(tools: &[&str]) -> std::result::Result<String, String> {
    let missing: Vec<&str> = tools
        .iter()
        .copied()
        .filter(|t| !crate::detect::tool_available(t))
        .collect();
    if missing.is_empty() {
        Ok(tools.join(", "))
    } else {
        Err(format!("missing from PATH: {}", missing.join(", ")))
    }
}

/// Download with retries (for a freshly published crate on the CDN).
#[cfg(any(feature = "publish-aur", feature = "publish-copr"))]
fn download(url: &str) -> Result<Vec<u8>> {
    curl(url, 6)
}

/// Single attempt, for pre-flight checks that shouldn't stall.
#[cfg(feature = "publish-aur")]
fn fetch(url: &str) -> Result<Vec<u8>> {
    curl(url, 0)
}

#[cfg(any(feature = "publish-aur", feature = "publish-copr"))]
fn curl(url: &str, retries: u32) -> Result<Vec<u8>> {
    use std::process::Command;

    use crate::errors::ReleaseError;

    let retries = retries.to_string();
    let output = Command::new("curl")
        .args([
            "-sSLf",
            "--retry",
            &retries,
            "--retry-delay",
            "10",
            "--retry-all-errors",
            url,
        ])
        .output()
        .map_err(|e| ReleaseError::Message(format!("spawning curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ReleaseError::Message(format!(
            "downloading {url} failed: {}",
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}
