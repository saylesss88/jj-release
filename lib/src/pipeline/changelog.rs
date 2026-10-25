//! Changelog generation

use std::{fs, path::Path};

use semver::Version;

use crate::{Config, ReleaseContext, Tag, bump, changelog, commits, detect, errors::Result};

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
        changelog::forge_base_url(config).and_then(|base| {
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
    let remote_info: Option<(String, String)> = if config.changelog.link_headers {
        changelog::forge_base_url(config).and_then(|base| {
            detect::remote_url(root) // Fetch the git URL string
                .and_then(|url| detect::parse_remote_owner_repo(&url)) // 2. Parse it
                .map(|(owner, repo)| (base, format!("{owner}/{repo}")))
        })
    } else {
        None
    };

    // Get all tags.
    let raw_tags = ctx.backend.list_tags()?;
    let mut all_tags: Vec<Tag> = raw_tags
        .into_iter()
        .filter_map(|name| {
            let stripped = name.strip_prefix(&config.release.tag_prefix)?;
            let version = Version::parse(stripped).ok()?;
            Some(Tag { name, version })
        })
        .collect();
    all_tags.sort_by(|a, b| a.version.cmp(&b.version));

    // Build sections, one per tag range.
    let mut sections = Vec::new();
    let mut versions: Vec<Option<Version>> = Vec::new();
    let mut urls = Vec::new();

    let mut prev = "root()".to_owned();
    let mut previous_version: Option<Version> = None;

    for tag in &all_tags {
        let revset = format!("{prev}..{}", tag.name);
        let tag_commits = ctx.backend.log_commits(&revset)?;

        let url = if let (Some(prev_v), Some((base, repo))) = (&previous_version, &remote_info) {
            Some(changelog::compare_url(
                base,
                repo,
                prev_v,
                &tag.version,
                &config.release.tag_prefix,
            ))
        } else {
            None // First tag has no previous tag to compare against
        };

        sections.push((tag.name.clone(), tag_commits));
        versions.push(Some(tag.version.clone()));
        urls.push(url);

        prev.clone_from(&tag.name);
        previous_version = Some(tag.version.clone());
    }

    let since = all_tags
        .last()
        .map_or_else(|| "root()".to_owned(), |t| t.name.clone());
    let tip_commits = ctx.backend.log_commits(&format!("{since}..@"))?;

    if !tip_commits.is_empty() {
        let url = if let (Some(prev_v), Some((base, repo))) = (&previous_version, &remote_info) {
            Some(changelog::compare_unreleased_url(
                base,
                repo,
                prev_v,
                &config.release.tag_prefix,
            ))
        } else {
            None
        };

        sections.push(("unreleased".to_owned(), tip_commits));
        versions.push(None);
        urls.push(url);
    }

    let result = changelog::render_full_changelog(&sections, &versions, &config.changelog, &urls);

    match output {
        Some(path) => {
            fs::write(path, &result)?;
            println!("✓ Written to {}", path.display());
        }
        None => print!("{result}"),
    }
    Ok(())
}
