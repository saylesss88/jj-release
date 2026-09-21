//! Release pipeline orchestration.

use std::{collections::HashMap, path::Path};

use semver::Version;

use crate::errors::{ReleaseError, Result};
use crate::{
    PreparedRelease,
    commits::{self, BumpKind},
    config::{Config, Versioning},
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

    let current_version = manifest
        .read_version(root)
        .map_err(|_| ReleaseError::Message("reading current version".into()))?;

    // Use crates.io version as baseline if available, more reliable than manifest.
    let baseline_version = if config.publish.cargo && !is_first_release {
        let cargo_toml = root.join("Cargo.toml");
        manifest::read_name(&cargo_toml)
            .ok()
            .and_then(|name| registry::latest_version_on_crates_io(&name).ok().flatten())
            .unwrap_or_else(|| current_version.clone())
    } else {
        current_version.clone()
    };

    let commits = backend.log_commits(&format!("{since}..@"))?;

    let mut bump = commits::resolve_bump(config.bump.force.as_ref(), &commits)?;

    let member_bumps = config.workspace.as_ref().map_or_else(
        || None,
        |ws| {
            if ws.enabled && matches!(ws.versioning, Versioning::Independent) {
                let versions = workspace::member_versions(root).ok();
                let bumps = workspace::member_bumps(
                    backend,
                    &ws.members,
                    &since,
                    config.bump.force.as_ref(),
                    &config.release.tag_prefix,
                )
                .ok();

                if let (Some(versions), Some(bumps)) = (versions, bumps) {
                    let mut map = HashMap::new();
                    for member in &ws.members {
                        let current = versions
                            .get(&member.name)
                            .cloned()
                            .unwrap_or_else(|| Version::new(0, 0, 0));
                        let member_bump =
                            bumps.get(&member.name).copied().unwrap_or(BumpKind::None);
                        let next = if is_first_release {
                            current.clone()
                        } else {
                            commits::apply_bump(&current, member_bump)
                        };
                        map.insert(member.name.clone(), (current, next));
                    }
                    Some(map)
                } else {
                    None
                }
            } else {
                None
            }
        },
    );

    // Upgrade bump to Major if cargo-semver-checks detects breaking changes.
    if !is_first_release
        && config.publish.cargo
        && config.publish.semver_checks
        && detect::tool_available("cargo-semver-checks")
    {
        eprintln!("→  Running cargo-semver-checks...");

        let is_stable = current_version.major >= 1;
        let has_breaking = publish::run_semver_checks(root)?;
        if has_breaking
            && bump < BumpKind::Major
            && (is_stable || config.publish.semver_checks_upgrade_major)
        {
            eprintln!(
                "warning: cargo-semver-checks detected API breaking changes, upgrading bump to Major"
            );
            bump = BumpKind::Major;
        }
    }

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
