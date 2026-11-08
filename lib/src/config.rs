//! `release.toml` config loading.

use std::{
    fmt::Write,
    fs,
    path::{Path, PathBuf},
};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{
    detect,
    errors::{ReleaseError, Result},
    workspace,
};

use jj_release_changelog::ChangelogConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub release: ReleaseConfig,
    pub bump: BumpConfig,
    pub publish: PublishConfig,
    pub changelog: ChangelogConfig,
    pub manifest_backend: String,
    pub workspace: Option<WorkspaceConfig>,
    pub replacements: Vec<Replacement>,
}

/// `[publish.aur]` section of the jj-release config.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AurConfig {
    /// AUR package name, e.g. `jj-release`.
    pub package: String,
    /// Git remote to push to. Defaults to `ssh://aur@aur.archlinux.org/<package>.git`.
    #[serde(default)]
    pub repo: Option<String>,
    /// Fail the release if this target can't be updated.
    ///
    /// When `false` (the default), a failed pre-flight check or publish for this
    /// target is reported as a warning and the release continues. When `true`,
    /// either one fails the release. Note that distro targets are updated after
    /// the crates.io publish, so a failure at that stage leaves the crate published.
    #[serde(default, alias = "required")]
    pub fail_on_error: bool,
}

impl AurConfig {
    #[cfg(feature = "publish-aur")]
    pub(crate) fn repo_url(&self) -> String {
        self.repo
            .clone()
            .unwrap_or_else(|| format!("ssh://aur@aur.archlinux.org/{}.git", self.package))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoprConfig {
    /// COPR project as `owner/project`, e.g. `saylesss88/jj-release`.
    pub project: String,
    /// Path to the RPM spec, relative to the repo root.
    pub spec: PathBuf,
    /// Wait for the COPR build to finish instead of returning right away.
    #[serde(default)]
    pub wait: bool,
    /// Fail the release if this target can't be updated.
    ///
    /// When `false` (the default), a failed pre-flight check or publish for this
    /// target is reported as a warning and the release continues. When `true`,
    /// either one fails the release. Note that distro targets are updated after
    /// the crates.io publish, so a failure at that stage leaves the crate published.
    #[serde(default, alias = "required")]
    pub fail_on_error: bool,
    /// Write the updated version back to the spec file in the repo after a
    /// successful publish. Defaults to true so the committed spec stays in sync.
    #[serde(default = "default_true")]
    pub update_local: bool,
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
    /// Release as the next pre-release on this channel (`alpha`, `beta`, `rc`)
    /// instead of a stable version.
    pub pre: Option<String>,
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
    pub aur: Option<AurConfig>,
    pub copr: Option<CoprConfig>,
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
            replacements: vec![],
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
            aur: None,
            copr: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Replacement {
    /// The file path to run the replacement in
    pub file: String,
    /// The regex search pattern
    pub search: String,
    /// The string to replace it with. Supports {{version}}, {{`crate_name`}}, and {{date}}
    pub replace: String,
    /// If true (default), the release will fail if the regex finds 0 matches or >1 match
    #[serde(default = "default_true")]
    pub exactly_one: bool,
}

const fn default_true() -> bool {
    true
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
pub fn generate_release_toml(root: &Path, tags: &[String]) -> String {
    let forge = detect::detect_forge(root).unwrap_or("github");
    let language = detect::detect_language(root).unwrap_or("cargo");

    let mut content = format!(
        r#"# Generated by jj-release

manifest_backend = "{language}"

[release]
forge = "{forge}"
create_release = false

[publish]
cargo = false

[changelog]
enabled = true
strict = true
# emoji_headers = true
# link_headers = true
# layout = "per-member"  # "root" (default) or "both"
# exclude_prefixes = ["chore", "ci", "test"]

# [changelog.prefix_mapping]
# security = "Security"
# doc = "Documentation"

# [[replacements]]
# file = "README.md"
# search = 'your-crate = ".*"'
# replace = 'your-crate = "{{{{version}}}}"'

# [publish.aur]
# package = "my-package"
# fail_on_error = false

# [publish.copr]
# Required: your COPR project as owner/project
# project = "your-username/my-cli-tool"
# fail_on_error = false

# Required: path to the spec, relative to the repo root
# spec = "packaging/rust-my-cli-tool.spec"

# Optional: wait for the COPR build to finish (default: false)
# wait = true
"#
    );

    if let Ok(Some(ws)) = workspace::detect_workspace(root) {
        let versioning = match ws.versioning {
            Versioning::Independent => "independent",
            Versioning::Unified => "unified",
        };

        let mut member_config = String::new();
        for member in &ws.members {
            let _ = write!(
                member_config,
                "\n[[workspace.members]]\nname = \"{}\"\npath = \"{}\"\n",
                member.name, member.path
            );
            if !member.publish {
                member_config.push_str("publish = false\n");
            }
            if !member.depends_on.is_empty() {
                let deps: Vec<String> = member
                    .depends_on
                    .iter()
                    .map(|d| format!("\"{d}\""))
                    .collect();
                let _ = writeln!(member_config, "depends_on = [{}]", deps.join(", "));
            }
            if versioning == "independent" {
                let prefix = workspace::detect_tag_prefix(member, tags)
                    .unwrap_or_else(|| format!("{}-v", member.name));
                let _ = writeln!(member_config, "tag_prefix = {prefix:?}");
            }
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
        let tags: Vec<String> = ["globset-0.4.21", "grep-0.4.1", "grep-cli-0.1.12", "15.2.0"]
            .map(String::from)
            .to_vec();
        let toml = generate_release_toml(dir.path(), &tags);
        assert!(toml.contains("forge ="));
        assert!(toml.contains("create_release = false"));
    }

    #[test]
    fn generate_release_toml_detects_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let tags: Vec<String> = ["v-0.4.21", "v-0.4.1", "v-0.1.12", "v-15.2.0"]
            .map(String::from)
            .to_vec();
        fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let toml = generate_release_toml(dir.path(), &tags);
        assert!(toml.contains("[workspace]"));
        assert!(toml.contains("enabled = true"));
    }
    #[test]
    fn generated_toml_sets_top_level_manifest_backend() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        let cfg: Config = toml::from_str(&generate_release_toml(dir.path(), &[])).unwrap();
        assert_eq!(cfg.manifest_backend, "npm");
    }

    #[test]
    fn resolves_glob_members_under_crates() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
        dir.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/scratch\"]\nresolver = \"2\"\n",
    ).unwrap();

        let crate_toml = |name: &str, extra: &str| {
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}"
            )
        };
        for (dir_name, manifest) in [
            ("core", crate_toml("core", "")),
            (
                "cli",
                crate_toml("cli", "\n[dependencies]\ncore = { path = \"../core\" }\n"),
            ),
            ("xtask", crate_toml("xtask", "publish = false\n")),
            ("scratch", crate_toml("scratch", "")),
        ] {
            let d = dir.path().join("crates").join(dir_name);
            fs::create_dir_all(d.join("src")).unwrap();
            fs::write(d.join("Cargo.toml"), manifest).unwrap();
            fs::write(d.join("src/lib.rs"), "").unwrap();
        }

        let mut members =
            workspace::parse_workspace_members(&dir.path().join("Cargo.toml")).unwrap();
        members.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();

        assert_eq!(names, ["cli", "core", "xtask"]); // scratch excluded
        let cli = members.iter().find(|m| m.name == "cli").unwrap();
        assert_eq!(cli.path, "crates/cli");
        assert_eq!(cli.depends_on, ["core"]);
        assert!(!members.iter().find(|m| m.name == "xtask").unwrap().publish);
    }
}
