use std::{fs, path::Path};

use anyhow::Result;
use jj_release::{
    changelog,
    commits::{self, BumpKind},
    config::{Config, Versioning},
    pipeline::{self, PreparedRelease, ReleaseContext},
    workspace,
};

pub fn release_pipeline(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let Some(prepared) = pipeline::prepare_release(ctx.backend, ctx.manifest, config, root)? else {
        info!("No trigger commit found or no releasable commits. Nothing to release.");
        return Ok(());
    };

    let PreparedRelease {
        next_version,
        tag_name,
        commits,
        ..
    } = &prepared;
    let is_independent = config
        .workspace
        .as_ref()
        .is_some_and(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent));

    info!("  Current version: {}", prepared.current_version);

    if !is_independent {
        info!(
            "  Bump: {:?} → {next_version}  (tag: {tag_name})",
            prepared.bump
        );
    }

    if dry_run {
        return print_dry_run(&prepared, config);
    }

    let is_independent = config
        .workspace
        .as_ref()
        .is_some_and(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent));

    if !is_independent {
        // 1. Write changelog.
        if config.changelog.enabled {
            info!("→ Writing changelog…");
            let changelog_path = root.join(&config.changelog.file);
            let existing = if changelog_path.exists() {
                fs::read_to_string(&changelog_path)?
            } else {
                String::new()
            };
            let section = changelog::render_changelog_section(commits, next_version);
            let updated = changelog::prepend_to_file(&existing, &section);
            fs::write(&changelog_path, updated)?;
        }

        // 2. Bump version and create release commit.
        info!("→ Bumping version to {next_version}…");
        ctx.manifest.write_version(root, next_version)?;
        let release_message = format!("chore: release {tag_name}");
        info!("→ Creating commit {:?}…", release_message);
        ctx.backend.new_commit(&release_message)?;

        // 3. Tag, bookmark, push.
        info!("→ Creating tag {tag_name}…");
        ctx.backend.create_tag(tag_name, "@")?;
        info!("→ Moving bookmark {:?} to @…", config.release.bookmark);
        ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
        info!("→ Exporting to git…");
        ctx.backend.git_export()?;
        info!("→ Pushing bookmark and tags…");
        ctx.backend
            .git_push(Some(&config.release.bookmark), Some(tag_name))?;
    }

    // 4. Publish.
    run_publish(ctx, config, root, &prepared, quiet)?;
    if is_independent {
        info!("→ Moving bookmark {:?} to @…", config.release.bookmark);
        ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
        info!("→ Exporting to git…");
        ctx.backend.git_export()?;
        info!("→ Pushing bookmark…");
        ctx.backend.git_push(Some(&config.release.bookmark), None)?;
    }
    // 5. Forge release.
    if !is_independent && config.release.create_release {
        info!("→ Creating forge release {tag_name}…");
        ctx.forge.create_release(tag_name)?;
    }

    info!("✓ Released {tag_name}");
    Ok(())
}

fn run_publish(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    prepared: &PreparedRelease,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let Some(ws) = &config.workspace else {
        info!("→ Publishing…");
        return ctx.publisher.publish(root, &config.publish.cargo_flags);
    };

    if !ws.enabled {
        info!("→ Publishing…");
        return ctx.publisher.publish(root, &config.publish.cargo_flags);
    }

    let ordered = workspace::ordered_members(&ws.members)?;

    match ws.versioning {
        Versioning::Unified => {
            for member in &ordered {
                info!("→ Publishing {}…", member.name);
                ctx.publisher
                    .publish(&root.join(&member.path), &config.publish.cargo_flags)?;
            }
        }
        Versioning::Independent => {
            let since = prepared.since.clone();
            let bumps = workspace::member_bumps(
                ctx.backend,
                &ws.members,
                &since,
                config.bump.force.as_ref(),
                &config.release.tag_prefix,
            )?;
            let versions = workspace::member_versions(root)?;
            for member in &ordered {
                let bump = bumps.get(&member.name).copied().unwrap_or(BumpKind::None);
                if bump == BumpKind::None {
                    info!("→ Skipping {} (no releasable commits)…", member.name);
                    continue;
                }
                let current = versions
                    .get(&member.name)
                    .cloned()
                    .unwrap_or_else(|| semver::Version::new(0, 0, 0));
                let next = commits::apply_bump(&current, bump);
                let tag = workspace::member_tag_name(member, &next, &config.release.tag_prefix);
                info!("→ Bumping {} to {next}…", member.name);
                workspace::bump_member_version(root, &member.name, &next)?;
                info!("→ Creating commit for {}…", member.name);
                ctx.backend.new_commit(&format!("chore: release {tag}"))?;
                info!("→ Creating tag {tag}...");
                ctx.backend.create_tag(&tag, "@")?;
                info!("→  Pushing tag {tag}...");
                ctx.backend.git_push(None, Some(&tag))?;
                info!("→ Publishing {}…", member.name);
                ctx.publisher
                    .publish(&root.join(&member.path), &config.publish.cargo_flags)?;
            }
        }
    }
    Ok(())
}

fn print_dry_run(prepared: &PreparedRelease, config: &Config) -> Result<()> {
    let is_independent = config
        .workspace
        .as_ref()
        .is_some_and(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent));

    if is_independent {
        println!("[dry-run] Independent workspace release:");
    } else {
        println!(
            "[dry-run] Would release {} as {}",
            prepared.next_version, prepared.tag_name
        );
    }

    if let Some(ws) = &config.workspace
        && ws.enabled
    {
        let ordered = workspace::ordered_members(&ws.members)?;
        println!("[dry-run] Would publish in order:");
        match ws.versioning {
            Versioning::Independent => {
                for member in ordered {
                    if let Some(ref mb) = prepared.member_bumps
                        && let Some((current, next)) = mb.get(&member.name)
                    {
                        if current == next {
                            println!(
                                "  - {} ({}) {} (no changes, skipping)",
                                member.name, member.path, current
                            );
                        } else {
                            let tag = workspace::member_tag_name(
                                member,
                                next,
                                &config.release.tag_prefix,
                            );
                            println!(
                                "  - {} ({}) {} → {} (tag: {})",
                                member.name, member.path, current, next, tag
                            );
                        }
                        continue;
                    }
                    println!("  - {} ({})", member.name, member.path);
                }
            }
            Versioning::Unified => {
                for member in ordered {
                    println!(
                        "  - {} ({}) → {}",
                        member.name, member.path, prepared.next_version
                    );
                }
            }
        }
        return Ok(());
    }

    println!("[dry-run] Would publish from root");
    Ok(())
}
