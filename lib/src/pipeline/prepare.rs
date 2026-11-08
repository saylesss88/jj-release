//! Release pipeline orchestration.

use std::{collections::HashMap, path::Path};

use semver::{Version, VersionReq};

use crate::{
    BumpKind, Config, ManifestBackend, MemberPlan, PreparedRelease, ReleasePlan, UnifiedPlan,
    Versioning, WorkspaceConfig, bump, commits, detect, errors::Result, jj::JjBackend, publish,
    workspace,
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

    // Independent: no repo-wide version is read at all.
    if let Some(ws) = independent_workspace(config) {
        let since = independent_since(backend, config, ws)?;
        if commits::find_trigger(backend, &config.release.trigger, &since)?.is_none() {
            return Ok(None);
        }
        let commits = backend.log_commits(&format!("{since}..@"))?;
        let members = plan_member_versions(backend, config, ws, root)?;

        // If no individual members have pending releases, abort the release early.
        if !members.values().any(super::MemberPlan::is_released) {
            return Ok(None);
        }

        return Ok(Some(PreparedRelease {
            since,
            commits,
            plan: ReleasePlan::Independent(members),
        }));
    }

    let since = resolve_since(backend, config)?;
    let is_first_release = since == "root()";

    // Check for trigger commit.
    if commits::find_trigger(backend, &config.release.trigger, &since)?.is_none() {
        return Ok(None);
    }

    let current_version = manifest.read_version(root)?;

    // Fetch the baseline from local VCS tags
    let latest_tag = commits::latest_version_tag(backend, &config.release.tag_prefix)?;

    // Use crates.io version as baseline if available, more reliable than manifest.
    let baseline_version = if is_first_release {
        current_version.clone()
    } else {
        latest_tag
            .as_ref()
            .map_or_else(|| current_version.clone(), |t| t.version.clone())
    };

    let commits = backend.log_commits(&format!("{since}..@"))?;

    let mut bump = commits::resolve_bump(config.bump.force.as_deref(), &commits)?;
    if !is_first_release {
        bump = upgrade_for_breaking_changes(config, root, &current_version, bump)?;
    }

    // Compute next_version. If it's the first release, freeze the current version.
    let next_version = if is_first_release {
        current_version.clone()
    } else {
        bump::next_version(
            latest_tag.as_ref().map(|t| &t.version),
            &current_version,
            bump,
        )
    };

    let tag_name = config.tag_name(&next_version);

    Ok(Some(PreparedRelease {
        since,
        commits,
        plan: ReleasePlan::Unified(UnifiedPlan {
            current_version,
            baseline_version,
            next_version,
            tag_name,
            bump,
        }),
    }))
}

/// Trigger search range for independent workspaces: since the most recent
/// release of any member, or the whole history if nothing has been tagged.
fn independent_since(
    backend: &dyn JjBackend,
    config: &Config,
    ws: &WorkspaceConfig,
) -> Result<String> {
    let tags = backend.list_tags("tags() & ::@")?;
    Ok(ws
        .members
        .iter()
        .filter_map(|m| {
            let prefix = m
                .tag_prefix
                .as_deref()
                .unwrap_or(&config.release.tag_prefix);
            tags.iter()
                .filter_map(|t| Some((t, Version::parse(t.strip_prefix(prefix)?).ok()?)))
                .max_by(|a, b| a.1.cmp(&b.1))
                .map(|(t, _)| t.clone())
        })
        // Pick the member tag that's newest in history, not highest version.
        .max_by_key(|t| backend.commit_date(t).unwrap_or_default())
        .unwrap_or_else(|| "root()".to_owned()))
}

/// The workspace config, if independent versioning is enabled.
#[must_use]
pub fn independent_workspace(config: &Config) -> Option<&WorkspaceConfig> {
    config
        .workspace
        .as_ref()
        .filter(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent))
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
) -> Result<HashMap<String, MemberPlan>> {
    let versions = workspace::member_versions(root)?;
    let bumps = workspace::member_bumps(
        backend,
        &ws.members,
        "root()",
        config.bump.force.as_deref(),
        &config.release.tag_prefix,
    )?;

    let mut plans = HashMap::new();
    for member in &ws.members {
        let current = versions
            .get(&member.name)
            .cloned()
            .unwrap_or_else(|| Version::new(0, 0, 0));

        // A member with no tags of its own is on its first release:
        // publish its current version as-is instead of bumping.
        let first_release =
            workspace::member_since(backend, member, &config.release.tag_prefix, "root()")?
                == "root()";

        let next = if first_release {
            current.clone()
        } else {
            let bump = bumps.get(&member.name).copied().unwrap_or(BumpKind::None);
            commits::apply_bump(&current, bump)
        };

        plans.insert(
            member.name.clone(),
            MemberPlan {
                current,
                next,
                first_release,
                propagated: false,
                updated_deps: Vec::new(),
            },
        );
    }
    let reqs = workspace::member_requirements(&root.join("Cargo.toml"))?;
    propagate_dependency_bumps(ws, &reqs, &mut plans)?;
    Ok(plans)
}

pub(super) fn resolve_since(backend: &dyn JjBackend, config: &Config) -> Result<String> {
    // If we have a previous release tag, start from there.
    if let Some(tag) = commits::latest_version_tag(backend, &config.release.tag_prefix)? {
        return Ok(tag.name);
    }

    eprintln!("hint: no version tag found. Entering First Release mode (starting from root).");
    Ok("root()".to_owned())
}

/// Release dependents whose requirement on a released member no longer
/// accepts that member's new version. Members are visited dependencies-first,
/// so a propagated release can cascade further down the graph.
fn propagate_dependency_bumps(
    ws: &WorkspaceConfig,
    reqs: &HashMap<String, HashMap<String, VersionReq>>,
    plans: &mut HashMap<String, MemberPlan>,
) -> Result<()> {
    for member in workspace::ordered_members(&ws.members)? {
        let Some(member_reqs) = reqs.get(&member.name) else {
            continue;
        };

        let mut outdated: Vec<String> = member_reqs
            .iter()
            .filter(|(dep, req)| {
                plans
                    .get(*dep)
                    .is_some_and(|p| p.is_released() && !req.matches(&p.next))
            })
            .map(|(dep, _)| dep.clone())
            .collect();
        if outdated.is_empty() {
            continue;
        }
        outdated.sort();

        let Some(plan) = plans.get_mut(&member.name) else {
            continue;
        };
        if !plan.is_released() {
            plan.next = commits::apply_bump(&plan.current, BumpKind::Patch);
            plan.propagated = true;
        }
        plan.updated_deps = outdated;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, path::Path};

    use crate::{PublishBackend, WorkspaceMember};

    use semver::Version;

    use crate::{
        config,
        errors::{ReleaseError, Result},
        manifest,
        pipeline::validate,
        test_helpers::mock::MockBackend,
    };

    use jj_release_changelog::CommitInfo;

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

        let prepared = prepare_release(&backend, &manifest, &config, Path::new("/tmp"))
            .unwrap()
            .expect("a release should be pending");
        let plan = prepared
            .unified()
            .expect("default config should give a unified plan");

        assert_eq!(plan.next_version, Version::parse("0.2.0").unwrap());
        assert_eq!(plan.tag_name, "v0.2.0");
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

    fn member(name: &str, deps: &[&str]) -> WorkspaceMember {
        WorkspaceMember {
            name: name.into(),
            path: format!("crates/{name}"),
            depends_on: deps.iter().map(|d| (*d).to_owned()).collect(),
            ..Default::default()
        }
    }

    fn plan(current: &str, next: &str) -> MemberPlan {
        MemberPlan {
            current: Version::parse(current).unwrap(),
            next: Version::parse(next).unwrap(),
            first_release: false,
            propagated: false,
            updated_deps: Vec::new(),
        }
    }

    fn reqs(entries: &[(&str, &str, &str)]) -> HashMap<String, HashMap<String, VersionReq>> {
        let mut out: HashMap<String, HashMap<String, VersionReq>> = HashMap::new();
        for (dependent, dep, req) in entries {
            out.entry((*dependent).to_owned())
                .or_default()
                .insert((*dep).to_owned(), VersionReq::parse(req).unwrap());
        }
        out
    }

    fn ws() -> WorkspaceConfig {
        WorkspaceConfig {
            enabled: true,
            versioning: Versioning::Independent,
            members: vec![
                member("globset", &[]),
                member("ignore", &["globset"]),
                member("ripgrep", &["ignore"]),
            ],
        }
    }

    #[test]
    fn breaking_bump_releases_direct_dependents() {
        let reqs = reqs(&[
            ("ignore", "globset", "0.4.21"),
            ("ripgrep", "ignore", "0.4.34"),
        ]);
        let mut plans = HashMap::from([
            ("globset".to_owned(), plan("0.4.21", "0.5.0")),
            ("ignore".to_owned(), plan("0.4.34", "0.4.34")),
            ("ripgrep".to_owned(), plan("15.2.0", "15.2.0")),
        ]);

        propagate_dependency_bumps(&ws(), &reqs, &mut plans).unwrap();

        let ignore = &plans["ignore"];
        assert_eq!(ignore.next, Version::new(0, 4, 35));
        assert!(ignore.propagated);
        assert_eq!(ignore.updated_deps, ["globset"]);

        // ignore's patch bump still satisfies ripgrep's "0.4.34" requirement.
        assert!(!plans["ripgrep"].is_released());
    }

    #[test]
    fn compatible_bump_does_not_propagate() {
        let reqs = reqs(&[("ignore", "globset", "0.4.21")]);
        let mut plans = HashMap::from([
            ("globset".to_owned(), plan("0.4.21", "0.4.22")),
            ("ignore".to_owned(), plan("0.4.34", "0.4.34")),
            ("ripgrep".to_owned(), plan("15.2.0", "15.2.0")),
        ]);

        propagate_dependency_bumps(&ws(), &reqs, &mut plans).unwrap();

        assert!(!plans["ignore"].is_released());
    }

    #[test]
    fn exact_pin_cascades() {
        let reqs = reqs(&[
            ("ignore", "globset", "0.4.21"),
            ("ripgrep", "ignore", "=0.4.34"),
        ]);
        let mut plans = HashMap::from([
            ("globset".to_owned(), plan("0.4.21", "0.5.0")),
            ("ignore".to_owned(), plan("0.4.34", "0.4.34")),
            ("ripgrep".to_owned(), plan("15.2.0", "15.2.0")),
        ]);

        propagate_dependency_bumps(&ws(), &reqs, &mut plans).unwrap();

        assert!(plans["ignore"].propagated);
        assert!(plans["ripgrep"].propagated);
        assert_eq!(plans["ripgrep"].next, Version::new(15, 2, 1));
    }

    #[test]
    fn own_bump_is_kept() {
        let reqs = reqs(&[("ignore", "globset", "0.4.21")]);
        let mut plans = HashMap::from([
            ("globset".to_owned(), plan("0.4.21", "0.5.0")),
            ("ignore".to_owned(), plan("0.4.34", "0.5.0")), // its own feat
            ("ripgrep".to_owned(), plan("15.2.0", "15.2.0")),
        ]);

        propagate_dependency_bumps(&ws(), &reqs, &mut plans).unwrap();

        let ignore = &plans["ignore"];
        assert_eq!(ignore.next, Version::new(0, 5, 0));
        assert!(!ignore.propagated);
        assert_eq!(ignore.updated_deps, ["globset"]);
    }
}
