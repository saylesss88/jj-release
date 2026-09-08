//! `release.toml` config loading.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub release: ReleaseConfig,
    pub bump: BumpConfig,
    pub publish: PublishConfig,
    pub changelog: ChangelogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReleaseConfig {
    /// Commit message substring that triggers a release.
    pub trigger: String,
    /// Prefix prepended to version numbers when creating tags.
    pub tag_prefix: String,
    /// Name of the bookmark to advance after a release.
    pub bookmark: String,
    /// Create a GitHub release via `gh release create`.
    pub github_release: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BumpConfig {
    /// Force a specific bump kind regardless of commit analysis.
    /// One of "major", "minor", "patch", or unset for automatic.
    pub force: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PublishConfig {
    /// Run `cargo publish` after tagging.
    pub cargo: bool,
    /// Extra flags forwarded to `cargo publish`.
    pub cargo_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChangelogConfig {
    pub enabled: bool,
    pub file: String,
}

impl Default for ChangelogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file: "CHANGELOG.md".to_owned(),
        }
    }
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            trigger: "Release: please".to_owned(),
            tag_prefix: "v".to_owned(),
            bookmark: "main".to_owned(),
            github_release: false,
        }
    }
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            cargo: true,
            cargo_flags: vec![],
        }
    }
}

/// Load config from `<root>/release.toml`, falling back to defaults if the
/// file doesn't exist.
pub fn load(root: &Path) -> Result<Config> {
    let path = root.join("release.toml");
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

impl Config {
    /// Format a version as a tag name, e.g. `"v1.2.3"`.
    pub fn tag_name(&self, version: &semver::Version) -> String {
        format!("{}{version}", self.release.tag_prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = Config::default();
        assert_eq!(cfg.release.trigger, "Release: please");
        assert_eq!(cfg.release.tag_prefix, "v");
        assert_eq!(cfg.release.bookmark, "main");
        assert!(cfg.publish.cargo);
    }

    #[test]
    fn tag_name_format() {
        let cfg = Config::default();
        let v = semver::Version::parse("1.2.3").unwrap();
        assert_eq!(cfg.tag_name(&v), "v1.2.3");
    }

    #[test]
    fn parse_minimal_toml() {
        let raw = r#"
[release]
trigger = "Ship it"
github_release = true

[publish]
cargo = false
"#;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(cfg.release.trigger, "Ship it");
        assert!(!cfg.publish.cargo);
        assert!(cfg.release.github_release);
        // Unspecified fields use defaults
        assert_eq!(cfg.release.tag_prefix, "v");
    }
}
