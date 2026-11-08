//! Commit walking and conventional commit parsing.

use git_conventional::Commit;
use semver::{Prerelease, Version};

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

/// Where a commit range starts: `tag`, or `root()` before the first release.
#[must_use]
pub(crate) fn since_revision(tag: Option<&Tag>) -> &str {
    tag.map_or("root()", |tag| tag.name.as_str())
}

// -- Pre-releases --

/// The next `<target>-<channel>.N` version: one past the highest existing
/// `<channel>.N` pre-release of `target`, or `.1` if there isn't one.
///
/// Moving up a channel (`beta` → `rc`) restarts at `.1`. Moving down
/// (`rc` → `beta`) is refused, since `1.0.0-beta.1` sorts below `1.0.0-rc.1`.
///
/// # Errors
///
/// Returns an error if `channel` isn't a single alphanumeric identifier,
/// `target` has already been released, or the new version wouldn't sort
/// above an existing pre-release of `target`.
pub fn next_prerelease(target: &Version, channel: &str, tags: &[Tag]) -> Result<Version> {
    validate_channel(channel)?;
    let core = release_core(target);

    let mut latest_pre: Option<&Version> = None;
    for version in tags.iter().map(|tag| &tag.version) {
        if release_core(version) != core {
            continue;
        }
        if version.pre.is_empty() {
            return Err(ReleaseError::Message(format!(
                "{core} has already been released"
            )));
        }
        latest_pre = latest_pre.max(Some(version));
    }

    let number = latest_pre
        .and_then(|v| {
            v.pre
                .as_str()
                .strip_prefix(channel)?
                .strip_prefix('.')?
                .parse::<u64>()
                .ok()
        })
        .map_or(1, |n| n + 1);

    let mut next = core.clone();
    next.pre = Prerelease::new(&format!("{channel}.{number}")).map_err(|e| {
        ReleaseError::Message(format!("invalid pre-release {channel}.{number}: {e}"))
    })?;

    if let Some(prev) = latest_pre.filter(|prev| next <= **prev) {
        return Err(ReleaseError::Message(format!(
            "{next} wouldn't sort above the existing {prev}; use a later channel or release {core}"
        )));
    }
    Ok(next)
}

/// `x.y.z` without pre-release or build metadata.
const fn release_core(version: &Version) -> Version {
    Version::new(version.major, version.minor, version.patch)
}

/// A channel must be one semver identifier, so `rc` works but not `rc.1`.
fn validate_channel(channel: &str) -> Result<()> {
    let valid = !channel.is_empty()
        && channel
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !channel.chars().all(|c| c.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(ReleaseError::Message(format!(
            "invalid pre-release channel {channel:?}: use a name like alpha, beta or rc"
        )))
    }
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

/// Before 1.0, a breaking change bumps the minor version (Cargo treats 0.x
/// minors as breaking). Reaching 1.0.0 takes `bump.force = "major"`.
#[must_use]
pub(crate) fn initial_development_bump(bump: BumpKind, baseline: &Version) -> BumpKind {
    if baseline.major == 0 && bump == BumpKind::Major {
        BumpKind::Minor
    } else {
        bump
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
    let mut next = Version::new(current.major, current.minor, current.patch);
    let pre = !current.pre.is_empty();
    match bump {
        BumpKind::Major if !(pre && next.minor == 0 && next.patch == 0) => {
            next.major += 1;
            next.minor = 0;
            next.patch = 0;
        }
        BumpKind::Minor if !(pre && next.patch == 0) => {
            next.minor += 1;
            next.patch = 0;
        }
        BumpKind::Patch if !pre => next.patch += 1,
        _ => {}
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::mock::MockBackend;

    fn tags(versions: &[&str]) -> Vec<Tag> {
        versions
            .iter()
            .map(|v| Tag {
                name: format!("v{v}"),
                version: Version::parse(v).unwrap(),
            })
            .collect()
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
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

    #[test]
    fn first_prerelease_is_numbered_one() {
        let next = next_prerelease(&v("1.0.0"), "rc", &tags(&["0.9.0"])).unwrap();
        assert_eq!(next, v("1.0.0-rc.1"));
    }

    #[test]
    fn prerelease_increments_numerically() {
        let existing = tags(&["0.9.0", "1.0.0-rc.9", "1.0.0-rc.10"]);
        assert_eq!(
            next_prerelease(&v("1.0.0"), "rc", &existing).unwrap(),
            v("1.0.0-rc.11")
        );
    }

    #[test]
    fn later_channel_restarts_numbering() {
        let existing = tags(&["1.0.0-beta.3"]);
        assert_eq!(
            next_prerelease(&v("1.0.0"), "rc", &existing).unwrap(),
            v("1.0.0-rc.1")
        );
    }

    #[test]
    fn earlier_channel_is_refused() {
        let existing = tags(&["1.0.0-rc.1"]);
        assert!(next_prerelease(&v("1.0.0"), "beta", &existing).is_err());
    }

    #[test]
    fn other_versions_prereleases_are_ignored() {
        let existing = tags(&["1.0.0", "2.0.0-rc.3"]);
        assert_eq!(
            next_prerelease(&v("1.1.0"), "rc", &existing).unwrap(),
            v("1.1.0-rc.1")
        );
    }

    #[test]
    fn prerelease_target_uses_its_core() {
        let existing = tags(&["1.0.0-rc.1"]);
        assert_eq!(
            next_prerelease(&v("1.0.0-rc.1"), "rc", &existing).unwrap(),
            v("1.0.0-rc.2")
        );
    }

    #[test]
    fn released_target_is_refused() {
        assert!(next_prerelease(&v("1.0.0"), "rc", &tags(&["1.0.0"])).is_err());
    }

    #[test]
    fn invalid_channels_are_refused() {
        for channel in ["", "rc.1", "123", "r c", "rc_1"] {
            assert!(
                next_prerelease(&v("1.0.0"), channel, &[]).is_err(),
                "{channel:?}"
            );
        }
    }
    #[test]
    fn bump_completes_prerelease() {
        let cases = [
            ("1.0.0-rc.1", BumpKind::Major, "1.0.0"),
            ("1.0.0-rc.1", BumpKind::Patch, "1.0.0"),
            ("1.2.0-rc.1", BumpKind::Minor, "1.2.0"),
            ("1.2.0-rc.1", BumpKind::Major, "2.0.0"),
            ("1.2.3-rc.1", BumpKind::Patch, "1.2.3"),
            ("1.2.3-rc.1", BumpKind::Minor, "1.3.0"),
            ("1.2.3-rc.1", BumpKind::None, "1.2.3"),
        ];
        for (from, bump, expected) in cases {
            assert_eq!(apply_bump(&v(from), bump), v(expected), "{from} {bump:?}");
        }
    }
    #[test]
    fn breaking_change_before_1_0_bumps_minor() {
        assert_eq!(
            initial_development_bump(BumpKind::Major, &v("0.10.0")),
            BumpKind::Minor
        );
        assert_eq!(
            initial_development_bump(BumpKind::Major, &v("0.11.0-rc.1")),
            BumpKind::Minor
        );
        assert_eq!(
            initial_development_bump(BumpKind::Major, &v("1.2.0")),
            BumpKind::Major
        );
        assert_eq!(
            initial_development_bump(BumpKind::Patch, &v("0.10.0")),
            BumpKind::Patch
        );
    }
}
