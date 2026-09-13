//! Release pipeline orchestration.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use semver::Version;

use crate::changelog;
use crate::commits::{self, BumpKind, CommitInfo};
use crate::config::Config;
use crate::forge::ForgeBackend;
use crate::jj::JjBackend;
use crate::manifest::ManifestBackend;
use crate::publish::PublishBackend;

pub struct PreparedRelease {
    pub since: String,
    pub current_version: Version,
    pub next_version: Version,
    pub tag_name: String,
    pub commits: Vec<CommitInfo>,
    pub bump: BumpKind,
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
/// use jj_release::pipeline::prepare_release;
/// use jj_release::manifest::CargoManifest;
/// use jj_release::config::Config;
/// use jj_release::jj::ShellBackend;
/// use std::path::Path;
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

    let since: String = if let Some(tag) =
        commits::latest_version_tag(backend, &config.release.tag_prefix)?
    {
        tag.name
    } else if config.changelog.require_tag {
        anyhow::bail!(
            "no version tag found\nhint: create a baseline tag first:\n  jj tag set v0.1.0 -r <your-last-release-commit>"
        );
    } else {
        "root()".to_owned()
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

    let next_version = commits::apply_bump(&current_version, bump);
    let tag_name = config.tag_name(&next_version);

    Ok(Some(PreparedRelease {
        since,
        current_version,
        next_version,
        tag_name,
        commits,
        bump,
    }))
}

/// Calculates and prints the upcoming version number to standard output.
///
/// # Errors
///
/// Returns an error if reading the version manifest, finding the latest tag,
/// or fetching commit logs fails.
pub fn print_next_version(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &std::path::Path,
) -> Result<()> {
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
    root: &std::path::Path,
    output: Option<&Path>,
) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commits::CommitInfo;
    use crate::jj::JjBackend;
    use anyhow::Result;
    use semver::Version;
    use std::cell::RefCell;
    use std::path::Path;

    struct MockBackend {
        tags: Vec<String>,
        commits: Vec<CommitInfo>,
        calls: RefCell<Vec<String>>,
    }

    impl JjBackend for MockBackend {
        fn list_tags(&self) -> Result<Vec<String>> {
            Ok(self.tags.clone())
        }
        fn log_commits(&self, revset: &str) -> Result<Vec<CommitInfo>> {
            self.calls.borrow_mut().push(revset.to_owned());
            Ok(self.commits.clone())
        }
        fn new_commit(&self, _: &str) -> Result<String> {
            Ok(String::new())
        }
        fn create_tag(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn set_bookmark(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn git_push(&self, _: &str, _: Option<&str>) -> Result<()> {
            Ok(())
        }
        fn git_export(&self) -> Result<()> {
            Ok(())
        }
        fn check_identity(&self) -> Result<()> {
            Ok(())
        }
    }

    struct MockManifest {
        version: Version,
    }

    impl crate::manifest::ManifestBackend for MockManifest {
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
        let config = crate::config::Config::default();
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
        let config = crate::config::Config::default();
        let prepared = prepare_release(&backend, &manifest, &config, Path::new("/tmp")).unwrap();
        assert!(prepared.is_none());
    }
}
