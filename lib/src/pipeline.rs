//! Release pipeline orchestration.

use std::{collections::HashMap, env, fs, path::Path};

use semver::Version;

use crate::changelog;
use crate::errors::{ReleaseError, Result};
use crate::{
    commits::{self, BumpKind, CommitInfo, Tag},
    config::{Config, Versioning},
    detect,
    forge::ForgeBackend,
    jj::JjBackend,
    manifest::{self, ManifestBackend},
    publish::{self, PublishBackend},
    registry, workspace,
};

#[non_exhaustive]
pub struct PreparedRelease {
    pub since: String,
    pub current_version: Version,
    pub baseline_version: Version,
    pub next_version: Version,
    pub tag_name: String,
    pub commits: Vec<CommitInfo>,
    pub bump: BumpKind,
    pub member_bumps: Option<HashMap<String, (Version, Version)>>,
}

#[non_exhaustive]
pub struct ReleaseContext<'a> {
    pub backend: &'a dyn JjBackend,
    pub manifest: &'a dyn ManifestBackend,
    pub forge: &'a dyn ForgeBackend,
    pub publisher: &'a dyn PublishBackend,
}

impl<'a> ReleaseContext<'a> {
    pub fn new(
        backend: &'a dyn JjBackend,
        manifest: &'a dyn ManifestBackend,
        forge: &'a dyn ForgeBackend,
        publisher: &'a dyn PublishBackend,
    ) -> Self {
        Self {
            backend,
            manifest,
            forge,
            publisher,
        }
    }
}

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

    if bump == BumpKind::None && !is_first_release {
        return Ok(None);
    }

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

/// Calculates and prints the upcoming version number to standard output.
///
/// # Errors
///
/// Returns an error if reading the version manifest, finding the latest tag,
/// or fetching commit logs fails.
pub fn print_next_version(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> Result<()> {
    let current = ctx.manifest.read_version(root)?;

    let since = resolve_since(ctx.backend, config)?;
    let commits = ctx.backend.log_commits(&format!("{since}..@"))?;
    let bump = commits::resolve_bump(config.bump.force.as_ref(), &commits)?;
    let next = commits::apply_bump(&current, bump);
    println!("{next}");
    Ok(())
}

/// Renders and prints the changelog section for the pending release to standard output.
///
/// # Errors
///
/// Returns an error if reading the version manifest, locating the latest release tag,
/// or fetching the commit history fails.
pub fn print_changelog(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    output: Option<&Path>,
    full: bool,
) -> Result<()> {
    if full {
        return print_full_changelog(ctx, config, root, output);
    }

    let current = ctx.manifest.read_version(root)?;

    let since = match commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };
    let commits = ctx.backend.log_commits(&format!("{since}..@"))?;
    let next = commits::apply_bump(&current, commits::compute_bump(&commits));
    let section = changelog::render_changelog_section(&commits, &next);

    match output {
        Some(path) => {
            let existing = if path.exists() {
                fs::read_to_string(path)?
            } else {
                String::new()
            };
            let updated = changelog::prepend_to_file(&existing, &section);
            fs::write(path, updated)?;
            println!("✓ Written to {}", path.display());
        }
        None => print!("{section}"),
    }
    Ok(())
}

fn print_full_changelog(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    output: Option<&Path>,
) -> Result<()> {
    // Get all tags.
    let raw_tags = ctx.backend.list_tags()?;
    let mut all_tags: Vec<Tag> = raw_tags
        .into_iter()
        .filter_map(|name| {
            let stripped = name.strip_prefix(&config.release.tag_prefix)?;
            let version = Version::parse(stripped).ok()?;
            Some(Tag { name, version })
        })
        .collect();
    all_tags.sort_by(|a, b| a.version.cmp(&b.version));

    // Build sections, one per tag range.
    let mut sections = Vec::new();
    let mut versions = Vec::new();
    let mut prev = "root()".to_owned();

    for tag in &all_tags {
        let revset = format!("{prev}..{}", tag.name);
        let tag_commits = ctx.backend.log_commits(&revset)?;
        sections.push((tag.name.clone(), tag_commits));
        versions.push(tag.version.clone());

        prev.clone_from(&tag.name);
    }

    // Include commits since last tag.
    let current = ctx.manifest.read_version(root)?;
    let since = all_tags
        .last()
        .map_or_else(|| "root()".to_owned(), |t| t.name.clone());
    let tip_commits = ctx.backend.log_commits(&format!("{since}..@"))?;
    let bump = commits::compute_bump(&tip_commits);
    let next = commits::apply_bump(&current, bump);
    sections.push(("unreleased".to_owned(), tip_commits));
    versions.push(next);

    let result = changelog::render_full_changelog(&sections, &versions);

    match output {
        Some(path) => {
            fs::write(path, &result)?;
            println!("✓ Written to {}", path.display());
        }
        None => print!("{result}"),
    }
    Ok(())
}

type CheckResult = Result<String, String>;

/// Validates the release environment, checking backend identity, forge CLI availability, manifest readability, version tags, trigger commits, and tokens.
///
/// # Errors
///
/// Returns an error if any backend operations fail, the manifest cannot be read, or a required version tag is missing when `require_tag` is enabled.
pub fn validate(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> Result<()> {
    let mut passed = 0;
    let mut failed = 0;

    let mut run_check = |label: &str, result: CheckResult| match result {
        Ok(msg) => {
            println!("✓ {label}: {msg}");
            passed += 1;
        }
        Err(msg) => {
            println!("✗ {label}: {msg}");
            failed += 1;
        }
    };

    let tag = commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)
        .map_err(|e| e.to_string());

    run_check("jj identity", check_jj_identity(ctx));
    run_check("forge CLI", check_forge_cli(config));
    run_check("manifest", check_manifest(ctx, root));
    run_check("version tag", check_version_tag(&tag));
    run_check(
        "publish pre-flight (dry-run)",
        check_publish(ctx.publisher, root),
    );

    if config.publish.cargo && config.publish.semver_checks {
        run_check("cargo-semver-checks", check_semver(ctx, config, root));
    }
    if config.publish.cargo
        && let Some(result) = check_crates_io(ctx, root)
    {
        run_check("crates.io", result);
    }

    let since = resolve_since(ctx.backend, config).map_err(|e| e.to_string());

    run_check(
        "trigger commit",
        since
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|s| check_trigger(ctx, config, s)),
    );

    run_check(
        "version bump",
        since
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|s| check_bump(ctx, config, s)),
    );

    if config.publish.cargo {
        run_check("CARGO_REGISTRY_TOKEN", check_cargo_token());
    }

    println!("\n{passed} passed, {failed} failed");
    if failed > 0 {
        return Err(ReleaseError::Message(format!("{failed} check(s) failed")));
    }
    Ok(())
}

fn resolve_since(backend: &dyn JjBackend, config: &Config) -> Result<String> {
    // If we have a previous release tag, start from there.
    if let Some(tag) = commits::latest_version_tag(backend, &config.release.tag_prefix)? {
        return Ok(tag.name);
    }

    eprintln!("hint: no version tag found. Entering First Release mode (starting from root).");
    Ok("root()".to_owned())
}

fn check_jj_identity(ctx: &ReleaseContext<'_>) -> CheckResult {
    ctx.backend
        .check_identity()
        .map(|()| "configured".to_owned())
        .map_err(|e| e.to_string())
}

fn check_forge_cli(config: &Config) -> CheckResult {
    match config.release.forge.as_str() {
        "github" if detect::tool_available("gh") => Ok("gh found".to_owned()),
        "github" => Err("gh not found. Install from https://cli.github.com".to_owned()),
        "gitlab" if detect::tool_available("glab") => Ok("glab found".to_owned()),
        "gitlab" => {
            Err("glab not found. Install from https://gitlab.com/gitlab-org/cli".to_owned())
        }
        _ => Ok("no forge CLI required".to_owned()),
    }
}

fn check_semver(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> CheckResult {
    if !detect::tool_available("cargo-semver-checks") {
        return Err("not found, install with: cargo install cargo-semver-checks".to_owned());
    }

    match publish::run_semver_checks(root) {
        Ok(false) => Ok("no breaking changes".to_owned()),
        Ok(true) => {
            let is_stable = ctx.manifest.read_version(root).is_ok_and(|v| v.major >= 1);
            let will_upgrade = is_stable || config.publish.semver_checks_upgrade_major;
            if will_upgrade {
                Err("breaking API changes detected, bump will be upgraded to Major".to_owned())
            } else {
                Ok("breaking API changes detected, skipping Major upgrade (pre-1.0)".to_owned())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

fn check_manifest(ctx: &ReleaseContext<'_>, root: &Path) -> CheckResult {
    ctx.manifest
        .read_version(root)
        .map(|v| format!("version {v}"))
        .map_err(|e| e.to_string())
}

fn check_version_tag(tag: &Result<Option<Tag>, String>) -> CheckResult {
    match tag {
        Ok(Some(t)) => Ok(format!("found {}", t.name)),
        Ok(None) => Ok("none (first release mode)".to_owned()),
        Err(e) => Err(e.clone()),
    }
}

fn check_crates_io(ctx: &ReleaseContext<'_>, root: &Path) -> Option<CheckResult> {
    let cargo_toml = root.join("Cargo.toml");
    let name = manifest::read_name(&cargo_toml).ok()?;
    let version = ctx.manifest.read_version(root).ok()?;

    let result = registry::latest_version_on_crates_io(&name)
        .map_err(|e| e.to_string())
        .map(|latest| match latest {
            Some(published) if published == version => {
                format!("v{version} published, in sync with manifest")
            }
            Some(published) => format!("v{published} published, manifest is v{version}"),
            None => "not yet published".to_owned(),
        });

    Some(result)
}

fn check_trigger(ctx: &ReleaseContext<'_>, config: &Config, since: &str) -> CheckResult {
    commits::find_trigger(ctx.backend, &config.release.trigger, since)
        .map_err(|e| e.to_string())
        .and_then(|t| t.ok_or_else(|| format!("no {:?} commit found", config.release.trigger)))
        .map(|_| "found".to_owned())
}

fn check_cargo_token() -> CheckResult {
    let has_env_token = env::var("CARGO_REGISTRY_TOKEN").is_ok();
    let has_credentials =
        dirs::home_dir().is_some_and(|h| h.join(".cargo/credentials.toml").exists());

    if has_env_token || has_credentials {
        Ok("configured".to_owned())
    } else {
        Err("not set and no ~/.cargo/credentials.toml found, needed for cargo publish".to_owned())
    }
}

fn check_publish(publisher: &dyn PublishBackend, root: &Path) -> CheckResult {
    publisher
        .check(root)
        .map(|()| "dry-run passed".to_owned())
        .map_err(|e| e.to_string())
}

fn check_bump(
    ctx: &crate::pipeline::ReleaseContext<'_>,
    config: &crate::config::Config,
    since: &str,
) -> Result<String, String> {
    // If it's a first release, we bypass the bump check completely!
    if since == "root()" {
        return Ok("first release mode (version frozen)".to_owned());
    }

    let commits = ctx
        .backend
        .log_commits(&format!("{since}..@"))
        .map_err(|e| e.to_string())?;

    let bump = crate::commits::resolve_bump(config.bump.force.as_ref(), &commits)
        .map_err(|e| e.to_string())?;

    if bump == crate::commits::BumpKind::None {
        Err("no releasable commits (feat/fix/BREAKING) found since last tag".to_owned())
    } else {
        Ok("releasable commits found".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, path::Path};

    use semver::Version;

    use crate::{
        commits::CommitInfo,
        config,
        errors::{ReleaseError, Result},
        manifest,
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
            check_publish(&publisher, Path::new("/tmp")),
            Ok("dry-run passed".to_owned())
        );
    }

    #[test]
    fn check_publish_failure() {
        let publisher = MockPublisher(true);
        assert_eq!(
            check_publish(&publisher, Path::new("/tmp")),
            Err("mock failure".to_owned())
        );
    }
}
