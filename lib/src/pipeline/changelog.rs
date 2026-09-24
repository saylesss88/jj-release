//! Changelog generation

use std::{fs, path::Path};

use semver::Version;

use crate::{Config, ReleaseContext, Tag, changelog, commits, errors::Result};

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

    let since = match commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };
    let commits = ctx.backend.log_commits(&format!("{since}..@"))?;
    let next = commits::apply_bump(&current, commits::compute_bump(&commits));
    let section = changelog::render_changelog_section(&commits, &next, true);

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
    let mut versions = Vec::new();
    let mut prev = "root()".to_owned();

    for tag in &all_tags {
        let revset = format!("{prev}..{}", tag.name);
        let tag_commits = ctx.backend.log_commits(&revset)?;
        sections.push((tag.name.clone(), tag_commits));
        versions.push(tag.version.clone());

        prev.clone_from(&tag.name);
    }

    // Include commits since last tag.
    let current = ctx.manifest.read_version(root)?;
    let since = all_tags
        .last()
        .map_or_else(|| "root()".to_owned(), |t| t.name.clone());
    let tip_commits = ctx.backend.log_commits(&format!("{since}..@"))?;
    if !tip_commits.is_empty() {
        let bump = commits::compute_bump(&tip_commits);
        let next = commits::apply_bump(&current, bump);
        sections.push(("unreleased".to_owned(), tip_commits));
        versions.push(next);
    }

    let result = changelog::render_full_changelog(&sections, &versions, config.changelog.strict);

    match output {
        Some(path) => {
            fs::write(path, &result)?;
            println!("✓ Written to {}", path.display());
        }
        None => print!("{result}"),
    }
    Ok(())
}
