use std::{
    fs, io,
    path::{Path, PathBuf},
};

use semver::Version;

use crate::{
    ReleasePlan, commits,
    config::{Config, Versioning, WorkspaceConfig, WorkspaceMember},
    errors::{ReleaseError, Result},
    pipeline::{self, CommitInfo, PreparedRelease, ReleaseContext},
    workspace,
};

use super::publish;
use jj_release_changelog::ChangelogLayout;

/// Prepends a new release section to the project's changelog.
///
/// If the configured changelog file does not exist, it will be created.
///
/// # Errors
///
/// Returns an error if the changelog file cannot be read from or written to disk.
pub fn update_changelog(
    ctx: &ReleaseContext<'_>,
    root: &Path,
    config: &Config,
    prepared: &pipeline::PreparedRelease,
) -> Result<()> {
    let Some(ws) = config.workspace.as_ref().filter(|w| w.enabled) else {
        // Single crate: layout doesn't apply
        return write_single_section(root, config, prepared);
    };
    let layout = config.changelog.layout;
    if layout.includes_root() {
        write_root_changelog(ctx, root, config, ws, prepared)?;
    }
    if layout.includes_members() {
        // Under Both, a member at "." shares the root file, which already has its changes.
        let skip_root_member = layout == ChangelogLayout::Both;
        write_member_changelogs(ctx, root, config, ws, prepared, skip_root_member)?;
    }
    Ok(())
}

fn write_single_section(root: &Path, config: &Config, prepared: &PreparedRelease) -> Result<()> {
    let plan = publish::unified_plan(prepared)?;
    write_section(
        &root_changelog(root, config),
        &jj_release_changelog::render_changelog_section(
            &prepared.commits,
            Some(&plan.next_version),
            &config.changelog,
            None,
        ),
    )
}

/// The root changelog file.
fn root_changelog(root: &Path, config: &Config) -> PathBuf {
    root.join(&config.changelog.file)
}

fn write_root_changelog(
    ctx: &ReleaseContext<'_>,
    root: &Path,
    config: &Config,
    ws: &WorkspaceConfig,
    prepared: &PreparedRelease,
) -> Result<()> {
    match ws.versioning {
        Versioning::Unified => write_single_section(root, config, prepared),
        Versioning::Independent => {
            let sections: Vec<String> = releasing_members(ctx, config, ws, prepared)?
                .into_iter()
                .map(|(member, version, commits)| {
                    let mut s = jj_release_changelog::render_heading(
                        Some(&member.name),
                        Some(&version),
                        None,
                        None,
                    );
                    s.push_str(&jj_release_changelog::render_body(
                        &commits,
                        &config.changelog,
                    ));
                    s.push_str(&publish::dependency_notes(member, prepared));
                    s
                })
                .collect();
            if sections.is_empty() {
                return Ok(());
            }
            write_section(&root_changelog(root, config), &sections.join("\n"))
        }
    }
}

fn write_member_changelogs(
    ctx: &ReleaseContext<'_>,
    root: &Path,
    config: &Config,
    ws: &WorkspaceConfig,
    prepared: &PreparedRelease,
    skip_root_member: bool,
) -> Result<()> {
    for (member, version, commits) in releasing_members(ctx, config, ws, prepared)? {
        if skip_root_member && member.path == "." {
            continue;
        }
        let body = jj_release_changelog::render_body(&commits, &config.changelog);
        let notes = publish::dependency_notes(member, prepared);
        if body.trim().is_empty() && notes.trim().is_empty() {
            continue;
        }

        let mut section = jj_release_changelog::render_heading(None, Some(&version), None, None);
        section.push_str(&body);
        section.push_str(&notes);

        let path =
            pipeline::changelog_cmd::member_changelog_path(root, member, &config.changelog.file);
        write_section(&path, &section)?;
    }
    Ok(())
}

/// Members being released in this run, with their next version and the
/// commits that touched their path. Members with no bump or no commits are
/// skipped.
fn releasing_members<'a>(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    ws: &'a WorkspaceConfig,
    prepared: &PreparedRelease,
) -> Result<Vec<(&'a WorkspaceMember, Version, Vec<CommitInfo>)>> {
    // Unified members share the root tag; look it up once.
    let root_since = commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)?
        .map_or_else(|| "root()".to_owned(), |t| t.name);

    let mut out = Vec::new();
    for member in &ws.members {
        let Some(version) = member_next_version(member, prepared) else {
            continue;
        };
        let since = match ws.versioning {
            Versioning::Unified => root_since.clone(),
            Versioning::Independent => {
                workspace::member_since(ctx.backend, member, &config.release.tag_prefix, "root()")?
            }
        };
        let fileset = workspace::member_fileset(member, &ws.members);
        let commits = ctx
            .backend
            .log_commits_for_path(&format!("{since}..@"), &fileset)?;
        let plan = prepared.members().and_then(|m| m.get(&member.name));
        let has_dep_updates = plan.is_some_and(|p| !p.updated_deps.is_empty());
        if !commits.is_empty() || has_dep_updates {
            out.push((member, version, commits));
        }
    }
    Ok(out)
}

/// The version `member` is being released as, or `None` if it isn't
/// being released (independent mode, no changes).
fn member_next_version(member: &WorkspaceMember, prepared: &PreparedRelease) -> Option<Version> {
    match &prepared.plan {
        ReleasePlan::Unified(plan) => Some(plan.next_version.clone()),
        ReleasePlan::Independent(members) => {
            let plan = members.get(&member.name)?;
            plan.is_released().then(|| plan.next.clone())
        }
    }
}

fn write_section(path: &Path, section: &str) -> Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        // Anything else must stop the release. Treating it as empty would
        // overwrite the existing changelog with just this section.
        Err(e) => {
            return Err(ReleaseError::Message(format!(
                "reading {}: {e}",
                path.display()
            )));
        }
    };
    if !existing.trim().is_empty() && !jj_release_changelog::has_changelog_title(&existing) {
        eprintln!(
            "warning: {} doesn't start with \"# Changelog\"; a Keep a Changelog header was added above its existing content",
            path.display()
        );
    }
    fs::write(
        path,
        jj_release_changelog::prepend_to_file(&existing, section),
    )
    .map_err(|e| ReleaseError::Message(format!("writing {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BumpKind, UnifiedPlan};

    #[test]
    fn single_crate_writes_the_configured_file() {
        let dir = tempfile::tempdir().unwrap();
        let prepared = PreparedRelease {
            since: "root()".into(),
            commits: vec![CommitInfo {
                change_id: "a".into(),
                description: "feat: thing".into(),
            }],
            plan: ReleasePlan::Unified(UnifiedPlan {
                current_version: Version::new(0, 1, 0),
                baseline_version: Version::new(0, 1, 0),
                next_version: Version::new(0, 2, 0),
                tag_name: "v0.2.0".into(),
                bump: BumpKind::Minor,
            }),
        };

        write_single_section(dir.path(), &Config::default(), &prepared).unwrap();

        let written = fs::read_to_string(dir.path().join("CHANGELOG.md")).unwrap();
        assert!(written.contains("## [0.2.0]"));
        assert!(written.contains("- thing"));
    }
}
