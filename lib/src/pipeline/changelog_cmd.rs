//! Changelog generation

use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use semver::Version;

use crate::{
    Config, ReleaseContext, Tag, WorkspaceMember, bump,
    commits::{self, CommitInfo},
    config::{Versioning, WorkspaceConfig},
    detect,
    errors::{ReleaseError, Result},
    manifest::{CargoManifest, ManifestBackend},
    workspace,
};

use jj_release_changelog::{self as changelog, ChangelogLayout, Release};

/// Where `print_changelog` sends its output.
#[derive(Debug, Clone, Copy)]
pub enum ChangelogOutput<'a> {
    /// Print to stdout.
    Stdout,
    /// Write to this exact file. Root layout only.
    Path(&'a Path),
    /// Write to the configured file(s): `[changelog].file` at the root,
    /// or in each member directory with `layout = "per-member"`.
    Configured,
}

/// The workspace, if the configured layout writes member changelogs.
fn member_layout_ws(config: &Config) -> Option<&WorkspaceConfig> {
    config
        .workspace
        .as_ref()
        .filter(|w| w.enabled && config.changelog.layout.includes_members())
}

/// The single file to write in root layout, or `None` for stdout.
fn root_target(config: &Config, root: &Path, target: ChangelogOutput<'_>) -> Option<PathBuf> {
    match target {
        ChangelogOutput::Stdout => None,
        ChangelogOutput::Path(p) => Some(p.to_path_buf()),
        ChangelogOutput::Configured => Some(root.join(&config.changelog.file)),
    }
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
    target: ChangelogOutput<'_>,
    full: bool,
) -> Result<()> {
    if member_layout_ws(config).is_some() && matches!(target, ChangelogOutput::Path(_)) {
        return Err(ReleaseError::Message(
            "--output writes a single file; with layout = \"per-member\" use --write".into(),
        ));
    }
    if full {
        return print_full_changelog(ctx, config, root, target);
    }

    let written = match member_layout_ws(config) {
        None => print_root_changelog(ctx, config, root, target, false)?,
        Some(ws) => {
            let layout = config.changelog.layout;
            let root_count = if layout.includes_root() {
                print_root_changelog(ctx, config, root, target, true)?
            } else {
                0
            };
            root_count
                + print_member_changelogs(
                    ctx,
                    config,
                    root,
                    ws,
                    target,
                    layout == ChangelogLayout::Both,
                )?
        }
    };

    if written == 0 {
        println!("No changelog entries since the last release.");
    }

    Ok(())
}

/// The root changelog section: one section for a single crate or unified
/// workspace, or named per-member sections for an independent workspace.
fn print_root_changelog(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    target: ChangelogOutput<'_>,
    show_header: bool,
) -> Result<usize> {
    if let Some(ws) = config
        .workspace
        .as_ref()
        .filter(|w| w.enabled && matches!(w.versioning, Versioning::Independent))
    {
        return print_independent_root(ctx, config, root, ws, target, show_header);
    }

    let current = ctx.manifest.read_version(root)?;
    let latest_tag = commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)?;
    let since = latest_tag.as_ref().map_or("root()", |t| t.name.as_str());
    let commits = ctx.backend.log_commits(&format!("{since}..@"))?;

    // None renders [Unreleased] (manifest not bumped past the latest tag).
    let version = section_version(latest_tag.as_ref(), &current, &commits);

    let url = remote_repo(config, root).and_then(|(base, repo)| {
        let prev = &latest_tag.as_ref()?.version;
        Some(version.as_ref().map_or_else(
            || changelog::compare_unreleased_url(&base, &repo, prev, &config.release.tag_prefix),
            |next| changelog::compare_url(&base, &repo, prev, next, &config.release.tag_prefix),
        ))
    });

    let section = changelog::render_changelog_section(
        &commits,
        version.as_ref(),
        &config.changelog,
        url.as_deref(),
    );

    if changelog::render_body(&commits, &config.changelog)
        .trim()
        .is_empty()
    {
        return Ok(0);
    }

    let path = root_target(config, root, target);
    let header = show_header.then(|| Path::new(&config.changelog.file));

    emit_pending(path.as_deref(), header, &section)?;
    Ok(1)
}

/// Generates and outputs a comprehensive changelog from the repository's entire history.
///
///# Errors
///
/// Returns an error if:
/// - The VCS backend fails to list tags or log commits.
/// - The local manifest version cannot be read.
/// - Writing to the provided `output` file fails.
pub fn print_full_changelog(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    target: ChangelogOutput<'_>,
) -> Result<()> {
    // Resolve remote base and repo only once
    let remote = remote_repo(config, root);

    let Some(ws) = member_layout_ws(config) else {
        return emit_root_full(ctx, config, root, target, remote.as_ref());
    };

    let layout = config.changelog.layout;
    let unified = matches!(ws.versioning, Versioning::Unified);
    // Unified: the root file is the aggregated history from `release.tag_prefix` tags.
    // Independent: a member at "." writes its own history to the root file instead.
    let aggregate_root = layout.includes_root() && unified;

    if aggregate_root {
        emit_root_full(ctx, config, root, target, remote.as_ref())?;
    } else if layout.includes_root() && !ws.members.iter().any(|m| m.path == ".") {
        eprintln!(
            "note: --full doesn't regenerate an aggregated root changelog for independent workspaces"
        );
    }

    for member in &ws.members {
        if aggregate_root && member.path == "." {
            continue;
        }
        let prefix = match ws.versioning {
            Versioning::Unified => config.release.tag_prefix.as_str(),
            Versioning::Independent => workspace::member_prefix(member, &config.release.tag_prefix),
        };
        let tags = sorted_tags(ctx, prefix)?;
        let fileset = workspace::member_fileset(member, &ws.members);
        let result =
            build_full_changelog(ctx, config, &tags, prefix, Some(&fileset), remote.as_ref())?;

        if matches!(target, ChangelogOutput::Configured) {
            let path = member_changelog_path(root, member, &config.changelog.file);
            write_full(&path, &result)?;
        } else {
            println!("==> {}/{} <==", member.path, config.changelog.file);
            print!("{result}");
        }
    }

    Ok(())
}

/// The complete root changelog, regenerated from every `release.tag_prefix` tag.
fn emit_root_full(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    target: ChangelogOutput<'_>,
    remote: Option<&(String, String)>,
) -> Result<()> {
    let tags = sorted_tags(ctx, &config.release.tag_prefix)?;
    let result =
        build_full_changelog(ctx, config, &tags, &config.release.tag_prefix, None, remote)?;
    emit_full(root_target(config, root, target).as_deref(), &result)
}

fn emit_pending(path: Option<&Path>, header: Option<&Path>, section: &str) -> Result<()> {
    if let Some(path) = path {
        return write_prepended(path, section);
    }
    if let Some(header) = header {
        println!("==> {} <==", header.display());
    }
    print!("{section}");
    Ok(())
}

fn emit_full(path: Option<&Path>, content: &str) -> Result<()> {
    path.map_or_else(
        || {
            print!("{content}");
            Ok(())
        },
        |path| write_full(path, content),
    )
}

/// All tags matching `prefix`, parsed and sorted oldest first.
fn sorted_tags(ctx: &ReleaseContext<'_>, prefix: &str) -> Result<Vec<Tag>> {
    let mut tags: Vec<Tag> = ctx
        .backend
        .list_tags()?
        .into_iter()
        .filter_map(|name| {
            let version = Version::parse(name.strip_prefix(prefix)?).ok()?;
            Some(Tag { name, version })
        })
        .collect();
    tags.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(tags)
}

fn log_commits(
    ctx: &ReleaseContext<'_>,
    revset: &str,
    path: Option<&str>,
) -> Result<Vec<CommitInfo>> {
    path.map_or_else(
        || ctx.backend.log_commits(revset),
        |p| ctx.backend.log_commits_for_path(revset, p),
    )
}

/// Render a full changelog for `tags`, optionally limited to commits
/// touching `path`. Empty ranges are skipped when a path is given, since
/// most releases won't touch every member.
fn build_full_changelog(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    tags: &[Tag],
    tag_prefix: &str,
    path: Option<&str>,
    remote: Option<&(String, String)>,
) -> Result<String> {
    let mut releases = Vec::new();

    let mut prev = "root()".to_owned();
    let mut previous_version: Option<Version> = None;

    for tag in tags {
        let commits = log_commits(ctx, &format!("{prev}..{}", tag.name), path)?;

        if !(path.is_some() && commits.is_empty()) {
            let compare_url = match (&previous_version, remote) {
                (Some(pv), Some((base, repo))) => Some(changelog::compare_url(
                    base,
                    repo,
                    pv,
                    &tag.version,
                    tag_prefix,
                )),
                _ => None,
            };
            releases.push(Release {
                version: Some(tag.version.clone()),
                date: Some(ctx.backend.commit_date(&tag.name)?),
                compare_url,
                commits,
            });
        }

        prev.clone_from(&tag.name);
        previous_version = Some(tag.version.clone());
    }

    let tip = log_commits(ctx, &format!("{prev}..@"), path)?;
    if !tip.is_empty() {
        let compare_url = match (&previous_version, remote) {
            (Some(pv), Some((base, repo))) => Some(changelog::compare_unreleased_url(
                base, repo, pv, tag_prefix,
            )),
            _ => None,
        };
        releases.push(Release {
            version: None,
            compare_url,
            commits: tip,
            ..Default::default()
        });
    }

    Ok(changelog::render_changelog(&releases, &config.changelog))
}

/// `None` renders `[Unreleased]` (manifest not bumped past the latest tag),
/// `Some` renders the next version.
fn section_version(
    latest: Option<&Tag>,
    current: &Version,
    commits: &[CommitInfo],
) -> Option<Version> {
    if latest.is_some_and(|t| *current <= t.version) {
        return None;
    }
    let bump = commits::compute_bump(commits);
    Some(bump::next_version(
        latest.map(|t| &t.version),
        current,
        bump,
    ))
}

fn print_member_changelogs(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    ws: &WorkspaceConfig,
    target: ChangelogOutput<'_>,
    skip_root_member: bool,
) -> Result<usize> {
    let mut written = 0;
    // Unified members share one version, computed from the whole repo.
    let unified_version = if matches!(ws.versioning, Versioning::Unified) {
        let current = ctx.manifest.read_version(root)?;
        let latest = commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)?;
        let since = latest.as_ref().map_or("root()", |t| t.name.as_str());
        let all = ctx.backend.log_commits(&format!("{since}..@"))?;
        Some(section_version(latest.as_ref(), &current, &all))
    } else {
        None
    };

    for member in &ws.members {
        if skip_root_member && member.path == "." {
            continue;
        }
        let prefix = match ws.versioning {
            Versioning::Unified => config.release.tag_prefix.as_str(),
            Versioning::Independent => workspace::member_prefix(member, &config.release.tag_prefix),
        };
        let latest = sorted_tags(ctx, prefix)?.pop();
        let since = latest.as_ref().map_or("root()", |t| t.name.as_str());
        let fileset = workspace::member_fileset(member, &ws.members);
        let commits = ctx
            .backend
            .log_commits_for_path(&format!("{since}..@"), &fileset)?;
        if changelog::render_body(&commits, &config.changelog)
            .trim()
            .is_empty()
        {
            continue;
        }
        if commits.is_empty() {
            continue;
        }

        let version = if let Some(v) = &unified_version {
            v.clone()
        } else {
            let current = CargoManifest.read_version(&root.join(&member.path))?;
            section_version(latest.as_ref(), &current, &commits)
        };

        let section = changelog::render_changelog_section(
            &commits,
            version.as_ref(),
            &config.changelog,
            None,
        );

        if let Some(path) = root_target(config, root, target) {
            write_prepended(&path, &section)?;
        } else {
            print!("{section}");
        }

        let header = Path::new(&member.path).join(&config.changelog.file);
        let path = matches!(target, ChangelogOutput::Configured)
            .then(|| member_changelog_path(root, member, &config.changelog.file));

        emit_pending(path.as_deref(), Some(&header), &section)?;

        written += 1;
    }
    Ok(written)
}

/// `(forge base URL, "owner/repo")` when `link_headers` is on and both can be determined.
fn remote_repo(config: &Config, root: &Path) -> Option<(String, String)> {
    if !config.changelog.link_headers {
        return None;
    }
    let base = changelog::forge_base_url(&config.release.forge, &config.release.forge_url)?;
    let (owner, repo) = detect::remote_url(root)
        .as_deref()
        .and_then(detect::parse_remote_owner_repo)?;
    Some((base, format!("{owner}/{repo}")))
}

/// Root layout, independent versioning: one named section per member with
/// changes since its own last tag, all in the root changelog.
fn print_independent_root(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    ws: &WorkspaceConfig,
    target: ChangelogOutput<'_>,
    show_header: bool,
) -> Result<usize> {
    let mut sections = Vec::new();

    for member in &ws.members {
        let prefix = workspace::member_prefix(member, &config.release.tag_prefix);
        let latest = sorted_tags(ctx, prefix)?.pop();
        let since = latest.as_ref().map_or("root()", |t| t.name.as_str());

        let fileset = workspace::member_fileset(member, &ws.members);
        let commits = ctx
            .backend
            .log_commits_for_path(&format!("{since}..@"), &fileset)?;
        if commits.is_empty() {
            continue;
        }

        let current = CargoManifest.read_version(&root.join(&member.path))?;
        let version = section_version(latest.as_ref(), &current, &commits);

        if let Some(section) =
            named_member_section(&member.name, version.as_ref(), &commits, config, "")
        {
            sections.push(section);
        }
    }

    if sections.is_empty() {
        return Ok(0);
    }
    let count = sections.len();
    let section = sections.join("\n");

    let path = root_target(config, root, target);
    let header = show_header.then(|| Path::new(&config.changelog.file));

    emit_pending(path.as_deref(), header, &section)?;

    Ok(count)
}

/// A named member section, or `None` if nothing in it would render
/// (e.g. every commit was filtered out by strict mode).
fn named_member_section(
    name: &str,
    version: Option<&Version>,
    commits: &[CommitInfo],
    config: &Config,
    extra: &str, // dependency notes, or "" in previews
) -> Option<String> {
    let body = changelog::render_body(commits, &config.changelog);
    if body.trim().is_empty() && extra.trim().is_empty() {
        return None;
    }
    let mut section = changelog::render_heading(Some(name), version, None, None);
    section.push_str(&body);
    section.push_str(extra);
    Some(section)
}

/// For `--full`: ask before replacing an existing changelog whose content
/// differs from what would be written. Non-interactive runs (CI, pipes)
/// refuse instead of overwriting.
fn confirm_overwrite(path: &Path, new: &str) -> Result<bool> {
    let Ok(existing) = fs::read_to_string(path) else {
        return Ok(true); // no file yet
    };
    if existing == new {
        return Ok(true); // nothing would change
    }
    if !io::stdin().is_terminal() {
        eprintln!(
            "skipping {}: it exists with different content (re-run interactively to confirm)",
            path.display()
        );
        return Ok(false);
    }
    eprint!(
        "{} exists and will be replaced. Continue? [y/N] ",
        path.display()
    );
    let _ = io::stderr().flush();
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Write a regenerated changelog, asking first if it would replace different content.
fn write_full(path: &Path, content: &str) -> Result<()> {
    if confirm_overwrite(path, content)? {
        fs::write(path, content)?;
        println!("✓ Written to {}", path.display());
    }
    Ok(())
}

/// The changelog file for `member`: `<root>/<file>` for a member at ".",
/// otherwise `<root>/<member path>/<file>`.
#[must_use]
pub fn member_changelog_path(root: &Path, member: &WorkspaceMember, file: &str) -> PathBuf {
    if member.path == "." {
        root.join(file)
    } else {
        root.join(&member.path).join(file)
    }
}

fn read_existing(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn write_prepended(path: &Path, section: &str) -> Result<()> {
    let existing = read_existing(path)?;

    if !existing.trim().is_empty() && !changelog::has_changelog_title(&existing) {
        eprintln!(
            "warning: {} doesn't start with \"# Changelog\"; \
            a Keep a Changelog header was added above its existing content",
            path.display()
        );
    }

    fs::write(path, changelog::prepend_to_file(&existing, section))?;
    println!("✔️ Written to {}", path.display());
    Ok(())
}
