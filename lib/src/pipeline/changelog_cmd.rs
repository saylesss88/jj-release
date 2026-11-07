//! Changelog generation

use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use jiff::Zoned;
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

use jj_release_changelog::{self as changelog, ChangelogLayout, ForgeRepo, Release};

// Output targets

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

impl ChangelogOutput<'_> {
    /// The root changelog file to write, or `None` to print.
    fn root_file(self, config: &Config, root: &Path) -> Option<PathBuf> {
        match self {
            Self::Stdout => None,
            Self::Path(path) => Some(path.to_path_buf()),
            Self::Configured => Some(root.join(&config.changelog.file)),
        }
    }

    /// `member`'s changelog file to write, or `None` to print.
    /// Only `Configured` writes member files; `--output` names a single file.
    fn member_file(self, root: &Path, member: &WorkspaceMember, file: &str) -> Option<PathBuf> {
        matches!(self, Self::Configured).then(|| member_changelog_path(root, member, file))
    }
}

/// `--output` names a single file, which can't hold per-member changelogs.
fn check_target(config: &Config, target: ChangelogOutput<'_>) -> Result<()> {
    if member_layout_ws(config).is_some() && matches!(target, ChangelogOutput::Path(_)) {
        return Err(ReleaseError::Message(
            "--output writes a single file; with layout = \"per-member\" use --write".into(),
        ));
    }
    Ok(())
}

// Entry points

/// Renders the changelog section(s) for the pending release and prints them
/// or prepends them to the changelog file(s), depending on `target`.
/// With `full`, regenerates the complete changelog instead.
///
/// # Errors
///
/// Returns an error if `target` is a single path but the layout writes member
/// changelogs, or if reading a manifest, listing tags, fetching commit
/// history, or writing a changelog file fails.
pub fn print_changelog(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    target: ChangelogOutput<'_>,
    full: bool,
) -> Result<()> {
    if full {
        return print_full_changelog(ctx, config, root, target);
    }
    check_target(config, target)?;

    let written = match member_layout_ws(config) {
        None => pending_root(ctx, config, root, target, false)?,
        Some(ws) => {
            let layout = config.changelog.layout;
            let root_count = if layout.includes_root() {
                pending_root(ctx, config, root, target, true)?
            } else {
                0
            };
            // With `Both`, the root changelog already covers a member at ".".
            let skip_root_member = layout == ChangelogLayout::Both;
            root_count + pending_members(ctx, config, root, ws, target, skip_root_member)?
        }
    };

    if written == 0 {
        println!("No changelog entries since the last release.");
    }

    Ok(())
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
    check_target(config, target)?;
    let forge = forge_repo(config, root);

    let Some(ws) = member_layout_ws(config) else {
        return full_root(ctx, config, root, target, forge.as_ref());
    };
    let includes_root = config.changelog.layout.includes_root();
    let aggregate_root = includes_root && matches!(ws.versioning, Versioning::Unified);

    if aggregate_root {
        full_root(ctx, config, root, target, forge.as_ref())?;
    } else if includes_root && !ws.members.iter().any(is_root_member) {
        eprintln!(
            "note: --full doesn't regenerate an aggregated root changelog for independent workspaces"
        );
    }

    let file = &config.changelog.file;
    for member in &ws.members {
        if aggregate_root && is_root_member(member) {
            continue;
        }
        let prefix = member_tag_prefix(config, ws, member);
        let fileset = workspace::member_fileset(member, &ws.members);
        let content = build_full_changelog(ctx, config, prefix, Some(&fileset), forge.as_ref())?;

        let header = member_display_path(member, file);
        emit(
            target.member_file(root, member, file).as_deref(),
            Some(&header),
            &content,
            WriteMode::Replace,
        )?;
    }

    Ok(())
}

/// A compare link from `prev` to `next`, or to the tip when `next` is `None`.
/// `None` without a forge repo or a previous release to compare against.
fn compare_link(
    forge: Option<&ForgeRepo>,
    prev: Option<&Version>,
    next: Option<&Version>,
    prefix: &str,
) -> Option<String> {
    Some(forge?.compare_link(prev?, next, prefix))
}

// Pending release

struct PendingChanges {
    latest: Option<Tag>,
    commits: Vec<CommitInfo>,
}

impl PendingChanges {
    /// Commits since the latest `prefix` tag (or all history before the first
    /// release), optionally limited to those touching `fileset`.
    fn since_last_tag(
        ctx: &ReleaseContext<'_>,
        prefix: &str,
        fileset: Option<&str>,
    ) -> Result<Self> {
        let latest = commits::latest_version_tag(ctx.backend, prefix)?;
        let since = latest.as_ref().map_or("root()", |tag| tag.name.as_str());
        let commits = log_commits(ctx, &format!("{since}..@"), fileset)?;
        Ok(Self { latest, commits })
    }

    fn latest_version(&self) -> Option<&Version> {
        self.latest.as_ref().map(|tag| &tag.version)
    }

    /// The version the pending section is headed with: `None` renders
    /// `[Unreleased]` (the manifest hasn't been bumped past the latest tag),
    /// `Some` is the next version.
    fn next_version(&self, current: &Version) -> Option<Version> {
        let latest = self.latest_version();
        if latest.is_some_and(|latest| current <= latest) {
            return None;
        }
        let bump = commits::compute_bump(&self.commits);
        Some(bump::next_version(latest, current, bump))
    }
}

/// Repo-wide changes since the last `release.tag_prefix` tag, with the
/// version they'd release as.
fn repo_pending(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
) -> Result<(PendingChanges, Option<Version>)> {
    let current = ctx.manifest.read_version(root)?;
    let pending = PendingChanges::since_last_tag(ctx, &config.release.tag_prefix, None)?;
    let version = pending.next_version(&current);
    Ok((pending, version))
}

/// Changes to `member` since its own last tag.
fn member_pending(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    ws: &WorkspaceConfig,
    member: &WorkspaceMember,
) -> Result<PendingChanges> {
    let fileset = workspace::member_fileset(member, &ws.members);
    PendingChanges::since_last_tag(ctx, member_tag_prefix(config, ws, member), Some(&fileset))
}

/// The root changelog section: one section for a single crate or unified
/// workspace, or named per-member sections for an independent workspace.
fn pending_root(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    target: ChangelogOutput<'_>,
    show_header: bool,
) -> Result<usize> {
    if let Some(ws) = independent_ws(config) {
        return pending_independent_root(ctx, config, root, ws, target, show_header);
    }

    let (pending, version) = repo_pending(ctx, config, root)?;
    let compare_url = compare_link(
        forge_repo(config, root).as_ref(),
        pending.latest_version(),
        version.as_ref(),
        &config.release.tag_prefix,
    );

    let Some(section) = changelog::render_pending_section(
        None,
        &pending.commits,
        version.as_ref(),
        Some(&today()),
        &config.changelog,
        compare_url.as_deref(),
    ) else {
        return Ok(0);
    };

    let header = show_header.then(|| Path::new(&config.changelog.file));
    emit(
        target.root_file(config, root).as_deref(),
        header,
        &section,
        WriteMode::Prepend,
    )?;
    Ok(1)
}

/// Root layout, independent versioning: one named section per member with
/// changes since its own last tag, all in the root changelog.
fn pending_independent_root(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    ws: &WorkspaceConfig,
    target: ChangelogOutput<'_>,
    show_header: bool,
) -> Result<usize> {
    let today = today();
    let mut sections = Vec::new();

    for member in &ws.members {
        let pending = member_pending(ctx, config, ws, member)?;

        if pending.commits.is_empty() {
            continue;
        }

        let version = pending.next_version(&member_current_version(root, member)?);
        sections.extend(changelog::render_pending_section(
            Some(&member.name),
            &pending.commits,
            version.as_ref(),
            Some(&today),
            &config.changelog,
            None,
        ));
    }

    if sections.is_empty() {
        return Ok(0);
    }
    let header = show_header.then(|| Path::new(&config.changelog.file));

    emit(
        target.root_file(config, root).as_deref(),
        header,
        &sections.join("\n"),
        WriteMode::Prepend,
    )?;

    Ok(sections.len())
}

fn pending_members(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    ws: &WorkspaceConfig,
    target: ChangelogOutput<'_>,
    skip_root_member: bool,
) -> Result<usize> {
    // Unified members all release at one version, computed from the whole repo.
    let unified_version = match ws.versioning {
        Versioning::Unified => Some(repo_pending(ctx, config, root)?.1),
        Versioning::Independent => None,
    };

    let today = today();
    let file = &config.changelog.file;
    let mut written = 0;

    for member in &ws.members {
        if skip_root_member && is_root_member(member) {
            continue;
        }
        let pending = member_pending(ctx, config, ws, member)?;
        if pending.commits.is_empty() {
            continue;
        }
        let version = match &unified_version {
            Some(version) => version.clone(),
            None => pending.next_version(&member_current_version(root, member)?),
        };
        let Some(section) = changelog::render_pending_section(
            None,
            &pending.commits,
            version.as_ref(),
            Some(&today),
            &config.changelog,
            None,
        ) else {
            continue;
        };

        let header = member_display_path(member, file);
        emit(
            target.member_file(root, member, file).as_deref(),
            Some(&header),
            &section,
            WriteMode::Prepend,
        )?;

        written += 1;
    }
    Ok(written)
}

// Full history

/// The complete root changelog, regenerated from every `release.tag_prefix` tag.
fn full_root(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    target: ChangelogOutput<'_>,
    forge: Option<&ForgeRepo>,
) -> Result<()> {
    let content = build_full_changelog(ctx, config, &config.release.tag_prefix, None, forge)?;
    emit(
        target.root_file(config, root).as_deref(),
        None,
        &content,
        WriteMode::Replace,
    )
}

/// Render a full changelog for `tags`, optionally limited to commits
/// touching `path`. Empty ranges are skipped when a path is given, since
/// most releases won't touch every member.
fn build_full_changelog(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    prefix: &str,
    fileset: Option<&str>,
    forge: Option<&ForgeRepo>,
) -> Result<String> {
    let tags = commits::version_tags(ctx.backend, prefix)?;
    let mut releases = Vec::new();
    let mut prev: Option<&Tag> = None;

    for tag in &tags {
        let since = prev.map_or("root()", |t| t.name.as_str());
        let commits = log_commits(ctx, &format!("{since}..{}", tag.name), fileset)?;

        if fileset.is_none() || !commits.is_empty() {
            releases.push(Release {
                version: Some(tag.version.clone()),
                date: Some(ctx.backend.commit_date(&tag.name)?),
                compare_url: compare_link(
                    forge,
                    prev.map(|t| &t.version),
                    Some(&tag.version),
                    prefix,
                ),
                commits,
            });
        }

        prev = Some(tag);
    }

    let since = prev.map_or("root()", |t| t.name.as_str());
    let tip = log_commits(ctx, &format!("{since}..@"), fileset)?;
    if !tip.is_empty() {
        releases.push(Release {
            version: None,
            compare_url: compare_link(forge, prev.map(|t| &t.version), None, prefix),
            commits: tip,
            ..Default::default()
        });
    }

    Ok(changelog::render_changelog(&releases, &config.changelog))
}

// Workspace helpers

fn enabled_ws(config: &Config) -> Option<&WorkspaceConfig> {
    config.workspace.as_ref().filter(|ws| ws.enabled)
}

/// The workspace, if the configured layout writes member changelogs.
fn member_layout_ws(config: &Config) -> Option<&WorkspaceConfig> {
    enabled_ws(config).filter(|_| config.changelog.layout.includes_members())
}

/// The workspace, if its members are versioned independently.
fn independent_ws(config: &Config) -> Option<&WorkspaceConfig> {
    enabled_ws(config).filter(|ws| matches!(ws.versioning, Versioning::Independent))
}

/// The tag prefix `member` releases under.
fn member_tag_prefix<'a>(
    config: &'a Config,
    ws: &WorkspaceConfig,
    member: &'a WorkspaceMember,
) -> &'a str {
    match ws.versioning {
        Versioning::Unified => &config.release.tag_prefix,
        Versioning::Independent => workspace::member_prefix(member, &config.release.tag_prefix),
    }
}

fn member_current_version(root: &Path, member: &WorkspaceMember) -> Result<Version> {
    CargoManifest.read_version(&root.join(&member.path))
}

fn is_root_member(member: &WorkspaceMember) -> bool {
    member.path == "."
}

/// The changelog file for `member`: `<root>/<file>` for a member at ".",
/// otherwise `<root>/<member path>/<file>`.
#[must_use]
pub fn member_changelog_path(root: &Path, member: &WorkspaceMember, file: &str) -> PathBuf {
    if is_root_member(member) {
        root.join(file)
    } else {
        root.join(&member.path).join(file)
    }
}

/// `<member path>/<file>`, for the `==> ... <==` header when printing.
fn member_display_path(member: &WorkspaceMember, file: &str) -> PathBuf {
    Path::new(&member.path).join(file)
}

// I/O

fn log_commits(
    ctx: &ReleaseContext<'_>,
    revset: &str,
    fileset: Option<&str>,
) -> Result<Vec<CommitInfo>> {
    fileset.map_or_else(
        || ctx.backend.log_commits(revset),
        |fileset| ctx.backend.log_commits_for_path(revset, fileset),
    )
}

/// How changelog content is written to an existing file.
#[derive(Debug, Clone, Copy)]
enum WriteMode {
    /// Insert a pending section above the existing entries.
    Prepend,
    /// Replace the whole file (asking first if it differs).
    Replace,
}

/// Write `content` to `path`, or print it (under `header`, if any) when `path` is `None`.
fn emit(path: Option<&Path>, header: Option<&Path>, content: &str, mode: WriteMode) -> Result<()> {
    let Some(path) = path else {
        if let Some(header) = header {
            println!("==> {} <==", header.display());
        }
        print!("{content}");
        return Ok(());
    };
    match mode {
        WriteMode::Prepend => write_prepended(path, content),
        WriteMode::Replace => write_full(path, content),
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

/// Write a regenerated changelog, asking first if it would replace different content.
fn write_full(path: &Path, content: &str) -> Result<()> {
    if confirm_overwrite(path, content)? {
        fs::write(path, content)?;
        println!("✓ Written to {}", path.display());
    }
    Ok(())
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

fn read_existing(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

/// The forge repo for compare links, when `link_headers` is on and both the
/// forge and the repo can be determined.
fn forge_repo(config: &Config, root: &Path) -> Option<ForgeRepo> {
    if !config.changelog.link_headers {
        return None;
    }
    let base = changelog::forge_base_url(&config.release.forge, &config.release.forge_url)?;
    let (owner, repo) = detect::remote_url(root)
        .as_deref()
        .and_then(detect::parse_remote_owner_repo)?;
    Some(ForgeRepo {
        base,
        repo: format!("{owner}/{repo}"),
    })
}

/// Todays date for pending release headings.
fn today() -> String {
    Zoned::now().date().to_string()
}
