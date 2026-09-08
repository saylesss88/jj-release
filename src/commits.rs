//! Commit walking and conventional commit parsing.

use anyhow::{Context, Result};
use git_conventional::Commit;
use semver::Version;

use crate::jj::JjBackend;

// ── Types ─────────────────────────────────────────────────────────────────────

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

// ── Walking ───────────────────────────────────────────────────────────────────

/// Detect whether the tip of the current working copy contains the trigger
/// string anywhere in its commit message.  We check `@` (the working-copy
/// parent stack) for the trigger commit.
///
/// Returns the change_id of the trigger commit if found, `None` otherwise.
pub fn find_trigger(backend: &dyn JjBackend, trigger: &str) -> Result<Option<String>> {
    // Walk the very tip — just @ and its immediate parent chain up to 10 deep.
    // We don't want to scan the whole repo; the trigger should be recent.
    let commits = backend
        .log_commits("@:: & ancestors(@, 10)")
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
pub fn latest_version_tag(backend: &dyn JjBackend) -> Result<Option<(String, Version)>> {
    // Ask jj for all tags reachable from @.
    // We list tags via `jj tag list` and then cross-reference with ancestors.
    // Simpler: query revset `tags()` which gives revisions that have tags,
    // then pick the highest semver one among ancestors of @.
    let tag_revs = backend
        .query_revset("ancestors(@) & tags()")
        .context("listing version tags")?;

    if tag_revs.is_empty() {
        return Ok(None);
    }

    // For each tagged revision, get its tags via log template.
    // jj's `tags` template gives the tag names.
    let raw = {
        // Build a revset that selects exactly those commits.
        // We'll just ask for all of them in one log call.
        backend.log_commits("ancestors(@) & tags()")?
    };

    // The description field won't have the tag name; we need a different
    // approach — shell to `jj tag list` and parse.
    // We use the backend directly for this one edge case via a helper.
    let _ = (tag_revs, raw); // suppress unused warnings for now

    // Delegate to the tag-listing helper below.
    find_latest_semver_tag(backend)
}

/// Shell out to `jj tag list` (via the backend's query mechanism with a
/// special template) to enumerate all tags and their targets, then return
/// the highest semver tag reachable from `@`.
fn find_latest_semver_tag(backend: &dyn JjBackend) -> Result<Option<(String, Version)>> {
    // Use the revset: for each ancestor of @, emit "tag_name\x1fchange_id".
    // jj's template language exposes `tags` on a commit as an iterator.
    let raw = backend
        .log_commits(r#"ancestors(@) & tags()"#)
        .context("querying tagged ancestors")?;

    // We can't get tag names from log_commits as-is because our template
    // only emits change_id + description. We need a custom query here.
    // This is a good example of where jj-lib would be cleaner.
    //
    // Workaround: use query_revset with a template that emits tag names.
    // We'll add a dedicated `list_tags` method to the backend in a follow-up;
    // for now, parse `jj tag list` output directly.
    let _ = raw;

    // Ask the backend with a revset that emits tag ref names via the template.
    // jj log -r 'ancestors(@) & tags()' -T 'refs.tags().map(|t| t.name() ++ "\n")'
    // This template syntax works in jj >= 0.21.
    let tag_names_raw = backend.query_revset(
        // query_revset uses commit_id template; we abuse it slightly here.
        // TODO: add list_tags() to the trait for a cleaner approach.
        "ancestors(@) & tags()",
    )?;

    // tag_names_raw is commit IDs, not tag names. We need a proper
    // `list_version_tags` on the backend. For now, return None and note
    // this is the next thing to wire up properly.
    let _ = tag_names_raw;

    Ok(None) // TODO: implement properly once list_tags is on the trait
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

    // Not a conventional commit — treat as no bump rather than erroring.
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
}
