use std::{fs, path::Path};

use semver::Version;
use toml_edit::DocumentMut;

use crate::{
    changelog,
    config::{Config, Versioning},
    errors::ReleaseError,
    errors::Result,
    manifest,
    pipeline::{self, PreparedRelease, ReleaseContext},
    workspace,
};

/// Executes pre-flight validation checks before mutating the repository.
///
/// For independent workspaces, members are checked in topological dependency order.
/// Otherwise, a single check is executed at the project root.
///
/// # Errors
///
/// Returns an error if a circular dependency is detected among workspace members,
/// or if the publisher's check mechanism (e.g., `cargo publish --dry-run`)
pub fn run_preflight_checks(
    ctx: &ReleaseContext<'_>,
    root: &Path,
    config: &Config,
    is_independent: bool,
) -> Result<()> {
    if is_independent {
        if let Some(ws) = &config.workspace {
            for member in workspace::ordered_members(&ws.members)? {
                ctx.publisher.check(&root.join(&member.path))?;
            }
        }
    } else {
        ctx.publisher.check(root)?;
    }
    Ok(())
}

/// Prepends a new release section to the project's changelog.
///
/// If the configured changelog file does not exist, it will be created.
///
/// # Errors
///
/// Returns an error if the changelog file cannot be read from or written to disk.
pub fn update_changelog(
    root: &Path,
    config: &Config,
    prepared: &pipeline::PreparedRelease,
) -> Result<()> {
    let changelog_path = root.join(&config.changelog.file);
    let existing = if changelog_path.exists() {
        fs::read_to_string(&changelog_path)?
    } else {
        String::new()
    };

    let section = changelog::render_changelog_section(
        &prepared.commits,
        &prepared.next_version,
        &config.changelog,
    );
    let updated = changelog::prepend_to_file(&existing, &section);
    fs::write(&changelog_path, updated)?;
    Ok(())
}

/// Synchronizes cross-member dependency versions within a Cargo workspace.
///
/// Iterates through all workspace members and updates any local dependencies
/// on other workspace members to match the upcoming release version.
///
/// # Errors
///
/// Returns an error if a member's `Cargo.toml` cannot be read, contains
/// invalid TOML syntax, or cannot be written back to disk.
pub fn update_workspace_dependencies(
    root: &Path,
    config: &Config,
    next_version: &Version,
) -> Result<()> {
    let Some(ws) = &config.workspace else {
        return Ok(());
    };
    if !ws.enabled {
        return Ok(());
    }

    let next_str = next_version.to_string();

    for member in &ws.members {
        let cargo_toml = root.join(&member.path).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }

        let raw = fs::read_to_string(&cargo_toml)?;
        let mut doc: DocumentMut = raw
            .parse()
            .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", cargo_toml.display())))?;

        for other in &ws.members {
            manifest::bump_workspace_dependency(&mut doc, &other.name, &next_str);
        }

        fs::write(&cargo_toml, doc.to_string())?;
    }

    Ok(())
}

/// Publishes the release to the configured package registry.
///
/// Routes the publish operation to either the unified or independent
/// workspace strategy based on the repository configuration. No-ops if
/// publishing is disabled.
///
/// # Errors
///
/// Returns an error if the underlying publish command (e.g., `cargo publish`)
/// fails, or if an independent workspace contains circular dependencies.
pub fn run_publish(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    prepared: &PreparedRelease,
) -> Result<()> {
    if !config.publish.cargo {
        return Ok(());
    }

    let is_independent = config
        .workspace
        .as_ref()
        .is_some_and(|ws| matches!(ws.versioning, Versioning::Independent));

    if is_independent {
        publish_independent(ctx, config, prepared, root)
    } else {
        publish_unified(ctx, config, root)
    }
}

pub(super) fn publish_independent(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    prepared: &PreparedRelease,
    root: &Path,
) -> Result<()> {
    let ws = config.workspace.as_ref().ok_or_else(|| {
        ReleaseError::Message("workspace config required for independent publishing".into())
    })?;
    let bumps = prepared.member_bumps.as_ref().ok_or_else(|| {
        ReleaseError::Message("member bumps required for independent publishing".into())
    })?;

    for member in workspace::ordered_members(&ws.members)? {
        let Some((current, next)) = bumps.get(&member.name) else {
            continue;
        };
        // Only publish if the topological member actually received a bump
        if current == next {
            continue;
        }
        let tag = workspace::member_tag_name(member, next, &config.release.tag_prefix);
        let member_root = root.join(&member.path);

        // Bump version
        workspace::bump_member_version(root, &member.name, next)?;

        // Update cross-member dependencies
        let cargo_toml = member_root.join("Cargo.toml");
        if cargo_toml.exists() {
            let raw = fs::read_to_string(&cargo_toml)?;
            let mut doc: DocumentMut = raw.parse().map_err(|e| {
                ReleaseError::Message(format!("parsing {}: {e}", cargo_toml.display()))
            })?;
            for other in &ws.members {
                if let Some((_, other_next)) = bumps.get(&other.name) {
                    manifest::bump_workspace_dependency(
                        &mut doc,
                        &other.name,
                        &other_next.to_string(),
                    );
                }
            }
            fs::write(&cargo_toml, doc.to_string())?;
        }

        // Commit, tag, export, push tag
        ctx.backend.new_commit(&format!("chore: release {tag}"))?;
        ctx.backend.create_tag(&tag, "@")?;
        ctx.backend.git_export()?;
        ctx.backend.git_push(None, Some(&tag))?;

        ctx.publisher
            .publish(&member_root, &config.publish.cargo_flags)?;
    }

    Ok(())
}

pub(super) fn publish_unified(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
) -> Result<()> {
    if let Some(ws) = config
        .workspace
        .as_ref()
        .filter(|w| w.enabled && !w.members.is_empty())
    {
        for member in workspace::ordered_members(&ws.members)?
            .into_iter()
            .filter(|m| m.publish)
        {
            ctx.publisher
                .publish(&root.join(&member.path), &config.publish.cargo_flags)?;
        }
        return Ok(());
    }

    // For single crates or unified workspaces, we just publish from the project root
    ctx.publisher.publish(root, &config.publish.cargo_flags)
}
