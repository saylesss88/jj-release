//! Commit walking and conventional commit parsing.

use git_conventional::Commit;
use semver::{BuildMetadata, Prerelease, Version};

use crate::errors::{ReleaseError, Result};
use crate::jj::JjBackend;

pub use jj_release_changelog::CommitInfo;

/// The kind of semver bump a commit implies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum BumpKind {
    #[default]
    None,
    Patch,
    Minor,
    Major,
}

/// A release tag and the version parsed from its name.
#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub version: Version,
}

// -- Tags --

/// Every tag named `<prefix><semver>`, sorted oldest first. Tags whose
/// remainder isn't valid semver are ignored.
///
/// # Errors
///
/// Returns an error if the backend fails to list tags.
pub(crate) fn version_tags(
    backend: &dyn JjBackend,
    prefix: &str,
    revset: &str,
) -> Result<Vec<Tag>> {
    let mut tags: Vec<Tag> = backend
        .list_tags(revset)?
        .into_iter()
        .filter_map(|name| {
            let version = Version::parse(name.strip_prefix(prefix)?).ok()?;
            Some(Tag { name, version })
        })
        .collect();
    tags.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(tags)
}

/// The highest-versioned `<prefix><semver>` tag in the repo, or `None`
/// before the first release.
///
/// Note: this considers every tag, not only those reachable from `@`.
///
/// # Errors
///
/// Returns an error if the backend fails to list tags.
pub fn latest_version_tag(backend: &dyn JjBackend, prefix: &str) -> Result<Option<Tag>> {
    Ok(version_tags(backend, prefix, "tags() & ::@")?.pop())
}

// -- Walking --

/// Find a commit in `since..@` whose description contains `trigger`.
///
/// Returns the trigger commit's `change_id`, or `None` if there isn't one.
pub(crate) fn find_trigger(
    backend: &dyn JjBackend,
    trigger: &str,
    since: &str,
) -> Result<Option<String>> {
    let commits = backend
        .log_commits(&format!("{since}..@"))
        .map_err(|e| ReleaseError::Message(format!("scanning for trigger commit: {e}")))?;

    Ok(commits
        .into_iter()
        .find(|c| c.description.contains(trigger))
        .map(|c| c.change_id))
}

/// Walk commits between `since_revision` (exclusive) and `@` (inclusive),
/// parse conventional commit messages, and return the required bump kind.
#[must_use]
pub(crate) fn compute_bump(commits: &[CommitInfo]) -> BumpKind {
    commits
        .iter()
        .map(|commit| classify(&commit.description))
        .max()
        .unwrap_or_default()
}

/// The bump to apply: the forced override if given, otherwise the one computed
/// from `commits`.
///
/// # Errors
///
/// Returns an error if `force` is not `"major"`, `"minor"`, or `"patch"`.
pub fn resolve_bump(force: Option<&str>, commits: &[CommitInfo]) -> Result<BumpKind> {
    match force {
        None => Ok(compute_bump(commits)),
        Some("major") => Ok(BumpKind::Major),
        Some("minor") => Ok(BumpKind::Minor),
        Some("patch") => Ok(BumpKind::Patch),
        Some(other) => Err(ReleaseError::Message(format!(
            "unknown bump.force value {other:?}: must be major/minor/patch"
        ))),
    }
}

/// Classify a single commit description. Non-conventional commits don't bump.
fn classify(description: &str) -> BumpKind {
    // Try parsing as a conventional commit first.
    let Ok(commit) = Commit::parse(description) else {
        return BumpKind::None;
    };
    if commit.breaking() {
        return BumpKind::Major;
    }
    match commit.type_().as_str() {
        "feat" => BumpKind::Minor,
        "fix" | "perf" | "refactor" => BumpKind::Patch,
        // docs, style, test, chore, ci, build → no bump
        _ => BumpKind::None,
    }
}

/// Apply a bump to a version, returning the new version.
#[must_use]
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
        BumpKind::Patch => next.patch += 1,

        BumpKind::None => {}
    }
    // Clear pre-release and build metadata on a real release.
    next.pre = Prerelease::EMPTY;
    next.build = BuildMetadata::EMPTY;
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::mock::MockBackend;

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

    #[test]
    fn resolve_bump_force_major() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "fix: small thing".into(),
        }];
        assert_eq!(
            resolve_bump(Some("major"), &commits).unwrap(),
            BumpKind::Major
        );
    }

    #[test]
    fn resolve_bump_force_overrides_commits() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "fix: small thing".into(),
        }];
        assert_eq!(
            resolve_bump(Some("minor"), &commits).unwrap(),
            BumpKind::Minor
        );
    }

    #[test]
    fn resolve_bump_auto_from_commits() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "feat: new thing".into(),
        }];
        assert_eq!(resolve_bump(None, &commits).unwrap(), BumpKind::Minor);
    }

    #[test]
    fn resolve_bump_unknown_force_errors() {
        assert!(resolve_bump(Some("banana"), &[]).is_err());
    }

    #[test]
    fn log_commits_for_path_uses_path_filter() {
        let backend = MockBackend {
            commits: vec![CommitInfo {
                change_id: "abc".into(),
                description: "feat: change lib".into(),
            }],
            ..MockBackend::default()
        };
        let commits = backend.log_commits_for_path("root()..@", "lib").unwrap();
        assert_eq!(commits.len(), 1);
        // verify the call was recorded
        assert_eq!(backend.calls.borrow()[0], "root()..@");
    }
}
