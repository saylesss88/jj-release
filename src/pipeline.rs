//! Release pipeline orchestration.

use std::{collections::HashMap, env, fmt::Display, fs, path::Path, process};

use anyhow::{Context, Result};
use semver::Version;

use crate::changelog;
use crate::{
    commits::{self, BumpKind, CommitInfo, Tag},
    config::{Config, Versioning},
    forge::ForgeBackend,
    jj::JjBackend,
    manifest::{self, ManifestBackend},
    publish::PublishBackend,
    registry, workspace,
};

pub struct PreparedRelease {
    pub since: String,
    pub current_version: Version,
    pub next_version: Version,
    pub tag_name: String,
    pub commits: Vec<CommitInfo>,
    pub bump: BumpKind,
    pub member_bumps: Option<HashMap<String, (Version, Version)>>,
}

pub struct ReleaseContext<'a> {
    pub backend: &'a dyn JjBackend,
    pub manifest: &'a dyn ManifestBackend,
    pub forge: &'a dyn ForgeBackend,
    pub publisher: &'a dyn PublishBackend,
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

    let since = match commits::latest_version_tag(backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => {
            if config.changelog.require_tag {
                // Auto-tag the current version as baseline instead of erroring.
                let current = manifest.read_version(root)?;
                let tag_name = format!("{}{current}", config.release.tag_prefix);
                eprintln!(
                    "hint: no version tag found, tagging current version {tag_name} as baseline"
                );
                backend.create_tag(&tag_name, "@-")?;
                backend.git_push(None, Some(&tag_name))?;
                tag_name
            } else {
                "root()".to_owned()
            }
        }
    };

    // Check for trigger commit.
    if commits::find_trigger(backend, &config.release.trigger, &since)?.is_none() {
        return Ok(None);
    }

    let current_version = manifest
        .read_version(root)
        .context("reading current version")?;

    let commits = backend.log_commits(&format!("{since}..@"))?;

    let bump = commits::resolve_bump(config.bump.force.as_ref(), &commits)?;

    if bump == BumpKind::None {
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
                        let bump = bumps.get(&member.name).copied().unwrap_or(BumpKind::None);
                        let next = commits::apply_bump(&current, bump);
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
    let next_version = commits::apply_bump(&current_version, bump);
    let tag_name = config.tag_name(&next_version);

    Ok(Some(PreparedRelease {
        since,
        current_version,
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

    let since: String = if let Some(tag) =
        commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)?
    {
        tag.name
    } else if config.changelog.require_tag {
        anyhow::bail!(
            "no version tag found\nhint: create a baseline tag first:\n  jj tag set v0.1.0 -r <your-last-release-commit>"
        );
    } else {
        "root()".to_owned()
    };
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
    // Actually get all tags.
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

/// Validates the release environment, checking backend identity, forge CLI availability, manifest readability, version tags, trigger commits, and tokens.
///
/// # Errors
///
/// Returns an error if any backend operations fail, the manifest cannot be read, or a required version tag is missing when `require_tag` is enabled.
pub fn validate(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> Result<()> {
    use crate::detect;

    let mut passed = 0;
    let mut failed = 0;

    macro_rules! check {
        ($label:expr, $result:expr) => {
            match $result {
                Ok(msg) => {
                    println!("✓ {}: {msg}", $label);
                    passed += 1;
                }
                Err(msg) => {
                    println!("✗ {}: {msg}", $label);
                    failed += 1;
                }
            }
        };
    }

    // jj identity.
    check!(
        "jj identity",
        ctx.backend
            .check_identity()
            .map(|()| "configured".to_owned())
            .map_err(|e| e.to_string())
    );

    // Forge CLI.
    let forge_check = match config.release.forge.as_str() {
        "github" => {
            if detect::tool_available("gh") {
                Ok("gh found".to_owned())
            } else {
                Err("gh not found. Install from https://cli.github.com".to_owned())
            }
        }
        "gitlab" => {
            if detect::tool_available("glab") {
                Ok("glab found".to_owned())
            } else {
                Err("glab not found. Install from https://gitlab.com/gitlab-org/cli".to_owned())
            }
        }
        _ => Ok("no forge CLI required".to_owned()),
    };
    check!("forge CLI", forge_check);

    // Manifest readable.
    check!(
        "manifest",
        ctx.manifest
            .read_version(root)
            .map(|v| format!("version {v}"))
            .map_err(|e| e.to_string())
    );

    // Version tag exists.
    check!(
        "version tag",
        commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)
            .map_err(|e| e.to_string())
            .and_then(|t| t.ok_or_else(|| "no version tag found. Create one first".to_owned()))
            .map(|t| format!("found {}", t.name))
    );
    // crates.io version check.
    if config.publish.cargo {
        let cargo_toml = root.join("Cargo.toml");
        if let Ok(name) = manifest::read_name(&cargo_toml)
            && let Ok(version) = ctx.manifest.read_version(root)
        {
            check!(
                "crates.io",
                registry::version_exists_on_crates_io(&name, &version)
                    .map_err(|e| e.to_string())
                    .and_then(|exists| if exists {
                        Err(format!(
                            "v{version} of {name} already published, bump the version"
                        ))
                    } else {
                        Ok(format!("v{version} of {name} not yet published"))
                    })
            );
        }
    }

    // Trigger commit.
    let since = if let Some(tag) =
        commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)?
    {
        tag.name
    } else {
        if config.changelog.require_tag {
            anyhow::bail!(
                "no version tag found\nhint: create a baseline tag first:\n  jj tag set v0.1.0 -r <your-last-release-commit>"
            );
        }
        "root()".to_owned()
    };
    check!(
        "trigger commit",
        commits::find_trigger(ctx.backend, &config.release.trigger, &since)
            .map_err(|e| e.to_string())
            .and_then(|t| t.ok_or_else(|| format!("no {:?} commit found", config.release.trigger)))
            .map(|_| "found".to_owned())
    );

    // CARGO_REGISTRY_TOKEN.
    if config.publish.cargo {
        let has_env_token = env::var("CARGO_REGISTRY_TOKEN").is_ok();
        let has_credentials =
            dirs::home_dir().is_some_and(|h| h.join(".cargo/credentials.toml").exists());

        check!(
            "CARGO_REGISTRY_TOKEN",
            if has_env_token || has_credentials {
                Ok("configured".to_owned())
            } else {
                Err(
                    "not set and no ~/.cargo/credentials.toml found, needed for cargo publish"
                        .to_owned(),
                )
            }
        );
    }

    println!();
    println!("{passed} passed, {failed} failed");

    if failed > 0 {
        process::exit(1);
    }
    Ok(())
}

pub fn info(quiet: bool, message: impl Display) {
    if !quiet {
        println!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, path::Path};

    use anyhow::Result;
    use semver::Version;

    use crate::{commits::CommitInfo, config, manifest, test_helpers::mock::MockBackend};

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
}
