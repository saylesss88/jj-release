//! `release.toml` config loading.

use std::{collections::HashMap, fs, path::Path};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::errors::{ReleaseError, Result};

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
#[non_exhaustive]
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
    pub semver_checks_upgrade_major: bool, // auto-upgrade to Major on breaking changes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(default)]
pub struct ChangelogConfig {
    pub enabled: bool,
    pub file: String,
    /// If true, silently drops commits that don't pass strict Conventional Commits parsing.
    /// If false, unformatted commits are placed in an "Other Changes" section.
    pub strict: bool,
    pub prefix_mapping: HashMap<String, String>,
    pub exclude_prefixes: Vec<String>,
    pub emoji_headers: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Versioning {
    #[default]
    Unified,
    Independent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
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
#[non_exhaustive]
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
            strict: true,
            prefix_mapping: HashMap::new(),
            exclude_prefixes: vec![],
            emoji_headers: false,
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
            cargo: false,
            cargo_flags: vec![],
            semver_checks: true,
            semver_checks_upgrade_major: false,
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
    let raw = fs::read_to_string(&path)
        .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;
    toml::from_str(&raw)
        .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", path.display())))
}

impl Config {
    /// Format a version as a tag name, e.g. `"v1.2.3"`.
    #[must_use]
    pub fn tag_name(&self, version: &Version) -> String {
        format!("{}{version}", self.release.tag_prefix)
    }
}

/// Generate a `release.toml` configuration string by auto-detecting
/// the forge, language, and workspace from the given root path.
#[must_use]
pub fn generate_release_toml(root: &Path) -> String {
    use crate::{detect, workspace};
    use std::fmt::Write;

    let forge = detect::detect_forge(root).unwrap_or("github");
    let language = detect::detect_language(root).unwrap_or("cargo");

    let mut content = format!(
        r#"# Generated by jj-release

[release]
forge = "{forge}"
create_release = false

[publish]
cargo = false

[changelog]
enabled = true
strict = true
# emoji_headers = true
# exclude_prefixes = ["chore", "ci", "test"]

# [changelog.prefix_mapping]
# security = "Security"
# doc = "Documentation"

manifest_backend = "{language}"
"#
    );

    if let Ok(Some(ws)) = workspace::detect_workspace(root) {
        let versioning = match ws.versioning {
            Versioning::Independent => "independent",
            Versioning::Unified => "unified",
        };

        let mut member_config = String::new();
        for member in &ws.members {
            let tag_prefix = if versioning == "independent" {
                format!("\ntag_prefix = \"{}-v\"", member.name)
            } else {
                String::new()
            };
            let _ = write!(
                member_config,
                "\n[[workspace.members]]\nname = \"{}\"\npath = \"{}\"\npublish = true{tag_prefix}\n# depends_on = []\n",
                member.name, member.path
            );
        }

        let _ = write!(
            content,
            "\n[workspace]\nenabled = true\nversioning = \"{versioning}\"\n{member_config}"
        );
    }

    content
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
        assert!(!cfg.publish.cargo);
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

    #[test]
    fn generate_release_toml_contains_forge() {
        let dir = tempfile::tempdir().unwrap();
        let toml = generate_release_toml(dir.path());
        assert!(toml.contains("forge ="));
        assert!(toml.contains("create_release = false"));
    }

    #[test]
    fn generate_release_toml_detects_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let toml = generate_release_toml(dir.path());
        assert!(toml.contains("[workspace]"));
        assert!(toml.contains("enabled = true"));
    }
}
