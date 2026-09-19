//! `release.toml` config loading.

use std::{fs, path::Path};

use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub release: ReleaseConfig,
    pub bump: BumpConfig,
    pub publish: PublishConfig,
    pub changelog: ChangelogConfig,
    pub manifest_backend: String,
    pub workspace: Option<WorkspaceConfig>,
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
    /// Forge to use
    pub forge: String,
    /// Forge URL
    pub forge_url: String,
    /// Whether to create a GH release when pushing a PR
    pub create_release: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BumpConfig {
    /// Force a specific bump kind regardless of commit analysis.
    /// One of "major", "minor", "patch", or unset for automatic.
    pub force: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(default)]
pub struct PublishConfig {
    /// Run `cargo publish` after tagging.
    pub cargo: bool,
    /// Extra flags forwarded to `cargo publish`.
    pub cargo_flags: Vec<String>,
    pub semver_checks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChangelogConfig {
    pub enabled: bool,
    pub file: String,
    pub require_tag: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Versioning {
    #[default]
    Unified,
    Independent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceMember {
    pub name: String,
    pub path: String,
    pub publish: bool,
    pub depends_on: Vec<String>,
    pub tag_prefix: Option<String>,
}

impl Default for WorkspaceMember {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: String::new(),
            publish: true,
            depends_on: vec![],
            tag_prefix: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WorkspaceConfig {
    pub enabled: bool,
    pub versioning: Versioning,
    pub members: Vec<WorkspaceMember>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            release: ReleaseConfig::default(),
            bump: BumpConfig::default(),
            publish: PublishConfig::default(),
            changelog: ChangelogConfig::default(),
            manifest_backend: "cargo".to_owned(),
            workspace: None,
        }
    }
}

impl Default for ChangelogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file: "CHANGELOG.md".to_owned(),
            require_tag: true,
        }
    }
}

impl Default for ReleaseConfig {
    fn default() -> Self {
        Self {
            trigger: "Release: please".to_owned(),
            tag_prefix: "v".to_owned(),
            bookmark: "main".to_owned(),
            forge: "github".to_owned(),
            forge_url: String::new(),
            create_release: false,
        }
    }
}

impl Default for PublishConfig {
    fn default() -> Self {
        Self {
            cargo: true,
            cargo_flags: vec![],
            semver_checks: true,
        }
    }
}

/// Loads release configuration from `<root>/release.toml`, falling back to
/// default values if the configuration file does not exist.
///
/// # Errors
///
/// Returns an error if the configuration file exists but cannot be read
/// (e.g., due to insufficient permissions) or if its contents contain invalid TOML syntax.
pub fn load(root: &Path) -> Result<Config> {
    let path = root.join("release.toml");
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

impl Config {
    /// Format a version as a tag name, e.g. `"v1.2.3"`.
    #[must_use]
    pub fn tag_name(&self, version: &Version) -> String {
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
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(cfg.tag_name(&v), "v1.2.3");
    }

    #[test]
    fn parse_minimal_toml() {
        let raw = r#"
[release]
trigger = "Ship it"
forge = "github"

[publish]
cargo = false
"#;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(cfg.release.trigger, "Ship it");
        assert!(!cfg.publish.cargo);
        assert_eq!(cfg.release.forge, "github");
        assert_eq!(cfg.release.tag_prefix, "v");
    }

    #[test]
    fn parse_forge_config() {
        let raw = r#"
[release]
forge = "github"
"#;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(cfg.release.forge, "github");
    }

    #[test]
    fn defaults_to_github_forge_no_release() {
        let cfg = Config::default();
        assert_eq!(cfg.release.forge, "github");
        assert!(!cfg.release.create_release);
    }

    #[test]
    fn member_tag_prefix_defaults_to_none() {
        let m = WorkspaceMember::default();
        assert!(m.tag_prefix.is_none());
    }

    #[test]
    fn parse_member_tag_prefix() {
        let raw = r#"
[workspace]
enabled = true
versioning = "independent"

[[workspace.members]]
name = "mylib"
path = "lib"
publish = true
tag_prefix = "mylib-v"
"#;
        let cfg: Config = toml::from_str(raw).unwrap();
        let member = &cfg.workspace.unwrap().members[0];
        assert_eq!(member.tag_prefix.as_deref(), Some("mylib-v"));
    }
}
