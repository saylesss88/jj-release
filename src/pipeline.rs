use std::path::Path;

use anyhow::{bail, Context, Result};
use semver::Version;

use crate::commits::{self, BumpKind, CommitInfo};
use crate::config::Config;
use crate::jj::JjBackend;
use crate::manifest::ManifestBackend;

pub struct PreparedRelease {
    pub since: String,
    pub current_version: Version,
    pub next_version: Version,
    pub tag_name: String,
    pub commits: Vec<CommitInfo>,
    pub bump: BumpKind,
}

pub fn prepare_release(
    backend: &dyn JjBackend,
    manifest: &dyn ManifestBackend,
    config: &Config,
    root: &Path,
) -> Result<Option<PreparedRelease>> {
    let since = match commits::latest_version_tag(backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };

    // Check for trigger commit.
    if commits::find_trigger(backend, &config.release.trigger, &since)?.is_none() {
        return Ok(None);
    }

    let current_version = manifest
        .read_version(root)
        .context("reading current version")?;

    let commits = backend.log_commits(&format!("{since}..@"))?;
    let bump = if let Some(force) = &config.bump.force {
        match force.as_str() {
            "major" => BumpKind::Major,
            "minor" => BumpKind::Minor,
            "patch" => BumpKind::Patch,
            other => bail!("unknown bump.force value {other:?} — must be major/minor/patch"),
        }
    } else {
        commits::compute_bump(&commits)
    };

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
            tags: vec![],
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
