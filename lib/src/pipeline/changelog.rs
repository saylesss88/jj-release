//! Changelog generation

use std::{fs, path::Path};

use semver::Version;

use crate::{
    Config, ReleaseContext, Tag, bump,
    commits::{self, CommitInfo},
    config::{Versioning, WorkspaceConfig},
    detect,
    errors::Result,
    manifest::{CargoManifest, ManifestBackend},
};

use jj_release_changelog::{ChangelogLayout, changelog};

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

    if let Some(ws) = config
        .workspace
        .as_ref()
        .filter(|w| w.enabled && config.changelog.layout == ChangelogLayout::PerMember)
    {
        return print_member_changelogs(ctx, config, root, ws, output);
    }

    let current = ctx.manifest.read_version(root)?;
    let latest_tag = commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)?;

    let since = latest_tag.as_ref().map_or("root()", |t| t.name.as_str());
    let commits = ctx.backend.log_commits(&format!("{since}..@"))?;

    // Determine if we should render [Unreleased] or a hard version.
    // If the manifest matches the tag, the user hasn't bumped it yet.
    let is_unreleased = latest_tag.as_ref().is_some_and(|t| current <= t.version);

    let bump = commits::compute_bump(&commits);

    let next = bump::next_version(latest_tag.as_ref().map(|t| &t.version), &current, bump);

    let target_version = if is_unreleased { None } else { Some(&next) };

    let url: Option<String> = if config.changelog.link_headers {
        jj_release_changelog::forge_base_url(&config.release.forge, &config.release.forge_url)
            .and_then(|base| {
                detect::remote_url(root)
                    .and_then(|url| detect::parse_remote_owner_repo(&url))
                    .map(|(owner, repo)| {
                        if let (true, Some(tag)) = (is_unreleased, &latest_tag) {
                            changelog::compare_unreleased_url(
                                &base,
                                &format!("{owner}/{repo}"),
                                &tag.version,
                                &config.release.tag_prefix,
                            )
                        } else {
                            changelog::compare_url(
                                &base,
                                &format!("{owner}/{repo}"),
                                &current,
                                &next,
                                &config.release.tag_prefix,
                            )
                        }
                    })
            })
    } else {
        None
    };

    let section = changelog::render_changelog_section(
        &commits,
        target_version,
        &config.changelog,
        url.as_deref(),
    );

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
    output: Option<&Path>,
) -> Result<()> {
    // Resolve remote base and repo only once
    let remote: Option<(String, String)> = if config.changelog.link_headers {
        jj_release_changelog::forge_base_url(&config.release.forge, &config.release.forge_url)
            .and_then(|base| {
                detect::remote_url(root) // Fetch the git URL string
                    .and_then(|url| detect::parse_remote_owner_repo(&url)) // 2. Parse it
                    .map(|(owner, repo)| (base, format!("{owner}/{repo}")))
            })
    } else {
        None
    };

    let per_member = config
        .workspace
        .as_ref()
        .filter(|w| w.enabled && config.changelog.layout == ChangelogLayout::PerMember);

    let Some(ws) = per_member else {
        // Root layout: unchanged behavior.
        let tags = sorted_tags(ctx, &config.release.tag_prefix)?;
        let result = build_full_changelog(
            ctx,
            config,
            &tags,
            &config.release.tag_prefix,
            None,
            remote.as_ref(),
        )?;
        return emit(output, &result);
    };

    for member in &ws.members {
        let prefix = match ws.versioning {
            Versioning::Unified => config.release.tag_prefix.as_str(),
            Versioning::Independent => member
                .tag_prefix
                .as_deref()
                .unwrap_or(&config.release.tag_prefix),
        };
        let tags = sorted_tags(ctx, prefix)?;
        let result = build_full_changelog(
            ctx,
            config,
            &tags,
            prefix,
            Some(&member.path),
            remote.as_ref(),
        )?;

        if output.is_some() {
            let path = root.join(&member.path).join(&config.changelog.file);
            fs::write(&path, &result)?;
            println!("✓ Written to {}", path.display());
        } else {
            println!("==> {}/{} <==", member.path, config.changelog.file);
            print!("{result}");
        }
    }

    Ok(())
}

fn emit(output: Option<&Path>, result: &str) -> Result<()> {
    match output {
        Some(path) => {
            fs::write(path, result)?;
            println!("✓ Written to {}", path.display());
        }
        None => print!("{result}"),
    }
    Ok(())
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
    let mut sections = Vec::new();
    let mut versions = Vec::new();
    let mut urls = Vec::new();

    let mut prev = "root()".to_owned();
    let mut previous_version: Option<Version> = None;

    for tag in tags {
        let commits = log_commits(ctx, &format!("{prev}..{}", tag.name), path)?;

        if !(path.is_some() && commits.is_empty()) {
            let url = match (&previous_version, remote) {
                (Some(pv), Some((base, repo))) => Some(changelog::compare_url(
                    base,
                    repo,
                    pv,
                    &tag.version,
                    tag_prefix,
                )),
                _ => None,
            };
            sections.push((tag.name.clone(), commits));
            versions.push(Some(tag.version.clone()));
            urls.push(url);
        }

        prev.clone_from(&tag.name);
        previous_version = Some(tag.version.clone());
    }

    let tip = log_commits(ctx, &format!("{prev}..@"), path)?;
    if !tip.is_empty() {
        let url = match (&previous_version, remote) {
            (Some(pv), Some((base, repo))) => Some(changelog::compare_unreleased_url(
                base, repo, pv, tag_prefix,
            )),
            _ => None,
        };
        sections.push(("unreleased".to_owned(), tip));
        versions.push(None);
        urls.push(url);
    }

    Ok(changelog::render_full_changelog(
        &sections,
        &versions,
        &config.changelog,
        &urls,
    ))
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
    output: Option<&Path>,
) -> Result<()> {
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
        let prefix = match ws.versioning {
            Versioning::Unified => config.release.tag_prefix.as_str(),
            Versioning::Independent => member
                .tag_prefix
                .as_deref()
                .unwrap_or(&config.release.tag_prefix),
        };
        let latest = sorted_tags(ctx, prefix)?.pop();
        let since = latest.as_ref().map_or("root()", |t| t.name.as_str());
        let commits = ctx
            .backend
            .log_commits_for_path(&format!("{since}..@"), &member.path)?;
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

        let file = root.join(&member.path).join(&config.changelog.file);
        if output.is_some() {
            let existing = fs::read_to_string(&file).unwrap_or_default();
            fs::write(&file, changelog::prepend_to_file(&existing, &section))?;
            println!("✓ Written to {}", file.display());
        } else {
            println!("==> {}/{} <==", member.path, config.changelog.file);
            print!("{section}");
        }
    }
    Ok(())
}
