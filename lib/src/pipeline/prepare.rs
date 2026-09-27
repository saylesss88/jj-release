//! Release pipeline orchestration.

use std::{collections::HashMap, path::Path};

use semver::Version;

use crate::errors::Result;
use crate::{
    PreparedRelease,
    commits::{self, BumpKind},
    config::{Config, Versioning, WorkspaceConfig},
    detect,
    jj::JjBackend,
    manifest::{self, ManifestBackend},
    publish, registry, workspace,
};

/// Prepares a new release by evaluating recent commits, checking for release triggers,
/// and calculating the next version bump based on configuration and the project manifest.
///
/// # Errors
///
/// Returns an error if querying repository tags, fetching commit logs, or reading
/// the project version manifest fails.
///
/// # Example
/// ```no_run
/// use std::path::Path;
/// use jj_release::{pipeline::prepare_release,
///     manifest::CargoManifest,
///     config::Config, jj::ShellBackend,
/// };
///
/// let backend = ShellBackend::new(Path::new(".")).unwrap();
/// let manifest = CargoManifest;
/// let config = Config::default();
/// let release = prepare_release(&backend, &manifest, &config, Path::new(".")).unwrap();
/// ```
pub fn prepare_release(
    backend: &dyn JjBackend,
    manifest: &dyn ManifestBackend,
    config: &Config,
    root: &Path,
) -> Result<Option<PreparedRelease>> {
    backend.check_identity()?;

    let since = resolve_since(backend, config)?;
    let is_first_release = since == "root()";

    // Check for trigger commit.
    if commits::find_trigger(backend, &config.release.trigger, &since)?.is_none() {
        return Ok(None);
    }

    let current_version = manifest.read_version(root)?;

    // Use crates.io version as baseline if available, more reliable than manifest.
    let baseline_version = if config.publish.cargo && !is_first_release {
        crates_io_version(root).unwrap_or_else(|| current_version.clone())
    } else {
        current_version.clone()
    };

    let commits = backend.log_commits(&format!("{since}..@"))?;

    let mut bump = commits::resolve_bump(config.bump.force.as_ref(), &commits)?;
    if !is_first_release {
        bump = upgrade_for_breaking_changes(config, root, &current_version, bump)?;
    }

    let member_bumps = independent_workspace(config)
        .map(|ws| plan_member_versions(backend, config, ws, root, &since, is_first_release))
        .transpose()?;

    // Compute next_version. If it's the first release, freeze the current version.
    let next_version = if is_first_release {
        current_version.clone()
    } else {
        commits::apply_bump(&baseline_version, bump)
    };

    let tag_name = config.tag_name(&next_version);

    Ok(Some(PreparedRelease {
        since,
        current_version,
        baseline_version,
        next_version,
        tag_name,
        commits,
        bump,
        member_bumps,
    }))
}

/// The workspace config, if independent versioning is enabled.
#[must_use]
pub fn independent_workspace(config: &Config) -> Option<&WorkspaceConfig> {
    config
        .workspace
        .as_ref()
        .filter(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent))
}

/// Latest published version on crates.io, if the crate is published and reachable.
fn crates_io_version(root: &Path) -> Option<Version> {
    let name = manifest::read_name(&root.join("Cargo.toml")).ok()?;
    registry::latest_version_on_crates_io(&name).ok().flatten()
}

/// Upgrade to a major bump if cargo-semver-checks finds breaking API changes.
fn upgrade_for_breaking_changes(
    config: &Config,
    root: &Path,
    current: &Version,
    bump: BumpKind,
) -> Result<BumpKind> {
    let enabled = config.publish.cargo
        && config.publish.semver_checks
        && detect::tool_available("cargo-semver-checks");
    // Pre-1.0 crates only upgrade when explicitly allowed.
    let may_upgrade = current.major >= 1 || config.publish.semver_checks_upgrade_major;

    if !enabled || !may_upgrade || bump >= BumpKind::Major {
        return Ok(bump);
    }

    eprintln!("→  Running cargo-semver-checks...");
    if publish::run_semver_checks(root)? {
        eprintln!(
            "warning: cargo-semver-checks detected API breaking changes, upgrading bump to Major"
        );
        return Ok(BumpKind::Major);
    }
    Ok(bump)
}

/// Current and next version for each member of an independently versioned workspace.
fn plan_member_versions(
    backend: &dyn JjBackend,
    config: &Config,
    ws: &WorkspaceConfig,
    root: &Path,
    since: &str,
    is_first_release: bool,
) -> Result<HashMap<String, (Version, Version)>> {
    let versions = workspace::member_versions(root)?;
    let bumps = workspace::member_bumps(
        backend,
        &ws.members,
        since,
        config.bump.force.as_ref(),
        &config.release.tag_prefix,
    )?;

    Ok(ws
        .members
        .iter()
        .map(|member| {
            let current = versions
                .get(&member.name)
                .cloned()
                .unwrap_or_else(|| Version::new(0, 0, 0));
            let next = if is_first_release {
                current.clone()
            } else {
                let bump = bumps.get(&member.name).copied().unwrap_or(BumpKind::None);
                commits::apply_bump(&current, bump)
            };
            (member.name.clone(), (current, next))
        })
        .collect())
}

pub(super) fn resolve_since(backend: &dyn JjBackend, config: &Config) -> Result<String> {
    // If we have a previous release tag, start from there.
    if let Some(tag) = commits::latest_version_tag(backend, &config.release.tag_prefix)? {
        return Ok(tag.name);
    }

    eprintln!("hint: no version tag found. Entering First Release mode (starting from root).");
    Ok("root()".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, path::Path};

    use crate::PublishBackend;

    use semver::Version;

    use crate::{
        commits::CommitInfo,
        config,
        errors::{ReleaseError, Result},
        manifest,
        pipeline::validate,
        test_helpers::mock::MockBackend,
    };

    struct MockManifest {
        version: Version,
    }

    impl manifest::ManifestBackend for MockManifest {
        fn read_version(&self, _root: &Path) -> Result<Version> {
            Ok(self.version.clone())
        }
        fn write_version(&self, _root: &Path, _version: &Version) -> Result<()> {
            Ok(())
        }
    }

    struct MockPublisher(bool);

    impl PublishBackend for MockPublisher {
        fn check(&self, _root: &Path) -> Result<()> {
            if self.0 {
                return Err(ReleaseError::Message("mock failure".to_string()));
            }
            Ok(())
        }
        fn publish(&self, _root: &Path, _flags: &[String]) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn prepare_release_computes_correct_next_version() {
        let backend = MockBackend {
            tags: vec!["v0.1.0".into()],
            commits: vec![
                CommitInfo {
                    change_id: "abc".into(),
                    description: "Release: please".into(),
                },
                CommitInfo {
                    change_id: "def".into(),
                    description: "feat: add thing".into(),
                },
            ],
            calls: RefCell::new(vec![]),
        };
        let manifest = MockManifest {
            version: Version::parse("0.1.0").unwrap(),
        };
        let config = config::Config::default();
        let prepared = prepare_release(&backend, &manifest, &config, Path::new("/tmp")).unwrap();
        assert!(prepared.is_some());
        let p = prepared.unwrap();
        assert_eq!(p.next_version, Version::parse("0.2.0").unwrap());
        assert_eq!(p.tag_name, "v0.2.0");
    }

    #[test]
    fn prepare_release_returns_none_when_no_trigger() {
        let backend = MockBackend {
            tags: vec!["v0.0.0".into()],
            commits: vec![CommitInfo {
                change_id: "abc".into(),
                description: "feat: add thing".into(),
            }],
            calls: RefCell::new(vec![]),
        };
        let manifest = MockManifest {
            version: Version::parse("0.0.0").unwrap(),
        };
        let config = Config::default();
        let prepared = prepare_release(&backend, &manifest, &config, Path::new("/tmp")).unwrap();
        assert!(prepared.is_none());
    }
    #[test]
    fn semver_checks_defaults_to_true() {
        let cfg = Config::default();
        assert!(cfg.publish.semver_checks);
    }

    #[test]
    fn parse_semver_checks_config() {
        let raw = r"
        [publish]
        semver_checks = false
        ";
        let cfg: Config = toml::from_str(raw).unwrap();
        assert!(!cfg.publish.semver_checks);
    }

    #[test]
    fn check_publish_success() {
        let publisher = MockPublisher(false);
        assert_eq!(
            validate::check_publish(&publisher, Path::new("/tmp")),
            Ok("dry-run passed".to_owned())
        );
    }

    #[test]
    fn check_publish_failure() {
        let publisher = MockPublisher(true);
        assert_eq!(
            validate::check_publish(&publisher, Path::new("/tmp")),
            Err("mock failure".to_owned())
        );
    }
}
