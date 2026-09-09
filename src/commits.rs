//! Commit walking and conventional commit parsing.

use anyhow::{Context, Result};
use git_conventional::Commit;
use semver::Version;

use crate::jj::JjBackend;

// -- Types --

/// Flat representation of a jj commit, as returned by the backend.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub change_id: String,
    pub description: String,
}

/// The kind of semver bump a commit implies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BumpKind {
    None,
    Patch,
    Minor,
    Major,
}

// -- Walking --

/// Detect whether the tip of the current working copy contains the trigger
/// string anywhere in its commit message.  We check `@` (the working-copy
/// parent stack) for the trigger commit.
///
/// Returns the `change_id` of the trigger commit if found, `None` otherwise.
pub fn find_trigger(backend: &dyn JjBackend, trigger: &str, since: &str) -> Result<Option<String>> {
    // Walk the very tip, just @ and its immediate parent chain up to 10 deep.
    // We don't want to scan the whole repo; the trigger should be recent.
    let commits = backend
        .log_commits(&format!("{since}..@"))
        .context("scanning for trigger commit")?;

    for c in commits {
        if c.description.contains(trigger) {
            return Ok(Some(c.change_id));
        }
    }
    Ok(None)
}

/// Return the latest `vX.Y.Z` tag reachable from `@`, as a `(tag_name,
/// Version)` pair.  Returns `None` if no version tag exists yet (first
/// release).
pub fn latest_version_tag(backend: &dyn JjBackend, prefix: &str) -> Result<Option<Tag>> {
    let raw_tags = backend.list_tags()?;
    let tags: Vec<Tag> = raw_tags
        .into_iter()
        .filter_map(|name| {
            let stripped = name.strip_prefix(prefix)?;
            let version = Version::parse(stripped).ok()?;
            Some(Tag { name, version })
        })
        .collect();
    Ok(highest_semver_tag(&tags).cloned())
}

/// Walk commits between `since_revision` (exclusive) and `@` (inclusive),
/// parse conventional commit messages, and return the required bump kind.
pub fn compute_bump(commits: &[CommitInfo]) -> BumpKind {
    let mut bump = BumpKind::None;

    for commit in commits {
        let kind = classify(&commit.description);
        if kind > bump {
            bump = kind;
        }
        // Short-circuit: can't go higher than Major.
        if bump == BumpKind::Major {
            break;
        }
    }
    bump
}

/// Classify a single commit description into a bump kind.
fn classify(description: &str) -> BumpKind {
    // Try parsing as a conventional commit first.
    if let Ok(conv) = Commit::parse(description) {
        if conv.breaking() {
            return BumpKind::Major;
        }
        return match conv.type_().as_str() {
            "feat" => BumpKind::Minor,
            "fix" | "perf" | "refactor" => BumpKind::Patch,
            // docs, style, test, chore, ci, build → no bump
            _ => BumpKind::None,
        };
    }

    // Not a conventional commit, treat as no bump rather than erroring.
    BumpKind::None
}

/// Apply a bump to a version, returning the new version.
pub fn apply_bump(current: &Version, bump: BumpKind) -> Version {
    let mut next = current.clone();
    match bump {
        BumpKind::Major => {
            next.major += 1;
            next.minor = 0;
            next.patch = 0;
        }
        BumpKind::Minor => {
            next.minor += 1;
            next.patch = 0;
        }
        BumpKind::Patch => {
            next.patch += 1;
        }
        BumpKind::None => {}
    }
    // Clear pre-release and build metadata on a real release.
    next.pre = semver::Prerelease::EMPTY;
    next.build = semver::BuildMetadata::EMPTY;
    next
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub version: Version,
}

pub fn highest_semver_tag(tags: &[Tag]) -> Option<&Tag> {
    tags.iter().max_by(|a, b| a.version.cmp(&b.version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
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

    #[test]
    fn latest_version_tag_finds_highest() {
        let backend = MockBackend {
            tags: vec!["v0.1.0".into(), "v0.3.0".into(), "v0.2.0".into()],
            ..MockBackend::default()
        };
        let result = latest_version_tag(&backend, "v").unwrap();
        assert_eq!(result.unwrap().name, "v0.3.0");
    }

    #[test]
    fn latest_version_tag_ignores_non_semver() {
        let backend = MockBackend {
            tags: vec![
                "latest".into(),
                "abc123".into(),
                "v0.2.0".into(),
                "def456".into(),
            ],
            ..MockBackend::default()
        };
        let result = latest_version_tag(&backend, "v").unwrap();
        assert_eq!(result.unwrap().name, "v0.2.0");
    }

    #[test]
    fn latest_version_tag_empty_returns_none() {
        let backend = MockBackend {
            tags: vec![],
            ..MockBackend::default()
        };
        let result = latest_version_tag(&backend, "v").unwrap();
        assert!(result.is_none());
    }
    fn bump(msg: &str) -> BumpKind {
        classify(msg)
    }

    #[test]
    fn conventional_feat_is_minor() {
        assert_eq!(bump("feat: add wallpaper cycling"), BumpKind::Minor);
    }

    #[test]
    fn conventional_fix_is_patch() {
        assert_eq!(bump("fix: prevent crash on empty dir"), BumpKind::Patch);
    }

    #[test]
    fn breaking_bang_is_major() {
        assert_eq!(bump("feat!: redesign IPC protocol"), BumpKind::Major);
    }

    #[test]
    fn breaking_footer_is_major() {
        assert_eq!(
            bump("refactor: change socket path\n\nBREAKING CHANGE: moved from /tmp to /run"),
            BumpKind::Major
        );
    }

    #[test]
    fn chore_is_none() {
        assert_eq!(bump("chore: bump deps"), BumpKind::None);
    }

    #[test]
    fn non_conventional_is_none() {
        assert_eq!(bump("did some stuff"), BumpKind::None);
    }

    #[test]
    fn apply_minor_bump() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(
            apply_bump(&v, BumpKind::Minor),
            Version::parse("1.3.0").unwrap()
        );
    }

    #[test]
    fn apply_major_bump() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(
            apply_bump(&v, BumpKind::Major),
            Version::parse("2.0.0").unwrap()
        );
    }

    #[test]
    fn compute_bump_takes_commits_directly() {
        let commits = vec![
            CommitInfo {
                change_id: "abc".into(),
                description: "feat: add thing".into(),
            },
            CommitInfo {
                change_id: "def".into(),
                description: "fix: patch thing".into(),
            },
        ];
        assert_eq!(compute_bump(&commits), BumpKind::Minor);
    }

    #[test]
    fn highest_semver_tag_picks_correct_version() {
        let tags = vec![
            Tag {
                name: "v0.1.0".into(),
                version: Version::parse("0.1.0").unwrap(),
            },
            Tag {
                name: "v0.3.0".into(),
                version: Version::parse("0.3.0").unwrap(),
            },
            Tag {
                name: "v0.2.0".into(),
                version: Version::parse("0.2.0").unwrap(),
            },
        ];
        let latest = highest_semver_tag(&tags).unwrap();
        assert_eq!(latest.name, "v0.3.0");
    }

    #[test]
    fn highest_semver_tag_empty_returns_none() {
        assert!(highest_semver_tag(&[]).is_none());
    }

    #[test]
    fn find_trigger_only_scans_since_last_tag() {
        let backend = MockBackend {
            commits: vec![CommitInfo {
                change_id: "abc".into(),
                description: "Release: please".into(),
            }],
            ..MockBackend::default()
        };
        find_trigger(&backend, "Release: please", "v0.1.0").unwrap();
        // should have called log_commits with "v0.1.0..@" not "root()..@"
        assert_eq!(backend.calls.borrow()[0], "v0.1.0..@");
    }
}
