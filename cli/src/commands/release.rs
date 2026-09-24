use std::io::IsTerminal;
use std::path::Path;

use jj_release_core::{
    config::{Config, Versioning},
    errors::Result,
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
        info!("");
        info!("hint: To trigger a release, create a commit with the exact message:");
        info!("      jj new -m {:?}", config.release.trigger);
        info!("      (Run `jj-release --help` for full usage details)");
        return Ok(());
    };

    let is_independent = config
        .workspace
        .as_ref()
        .is_some_and(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent));

    info!("  Current version: {}", prepared.current_version);
    if !is_independent && prepared.baseline_version != prepared.current_version {
        info!("  crates.io version: {}", prepared.baseline_version);
        info!("  (using crates.io version as bump baseline)");
    }

    if dry_run {
        return print_dry_run(&prepared, config);
    }

    // Run pre-flight checks on all targets before touching anything
    info!(" → Running pre-flight checks (dry-run)...");
    pipeline::run_preflight_checks(ctx, root, config, is_independent)?;

    // Warn if publishing is disabled, prompt interactively, skip in CI.
    if !config.publish.cargo {
        if std::io::stdin().is_terminal() {
            eprint!(
                "warning: publish.cargo = false. This will still bump versions, tag, and push, \
                 but will NOT publish to crates.io. Continue? [y/N] "
            );
            let _ = std::io::Write::flush(&mut std::io::stderr());
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                println!("Aborted.");
                return Ok(());
            }
        } else {
            eprintln!("warning: publish.cargo = false, skipping crates.io publish.");
        }
    }

    if !is_independent {
        // Write changelog.
        if config.changelog.enabled {
            info!("→ Writing changelog…");
            pipeline::update_changelog(root, config, &prepared)?;
        }

        // Bump version and create release commit.
        info!("→ Bumping version to {}…", prepared.next_version);
        ctx.manifest.write_version(root, &prepared.next_version)?;
        pipeline::update_workspace_dependencies(root, config, &prepared.next_version)?;

        let release_message = format!("chore: release {}", prepared.tag_name);
        info!("→ Creating commit {:?}…", release_message);
        ctx.backend.new_commit(&release_message)?;

        // Tag and bookmark locally
        info!("→ Creating tag {}…", prepared.tag_name);
        ctx.backend.create_tag(&prepared.tag_name, "@")?;

        info!("→ Moving bookmark {:?} to @…", config.release.bookmark);
        ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
        info!("→ Exporting to git…");
        ctx.backend.git_export()?;
    }

    // Publish to registry
    pipeline::run_publish(ctx, config, root, &prepared)?;

    // Push to remote
    if is_independent {
        info!("→ Moving bookmark {:?} to @…", config.release.bookmark);
        ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
        info!("→ Exporting to git…");
        ctx.backend.git_export()?;
        info!("→ Pushing bookmark…");
        ctx.backend.git_push(Some(&config.release.bookmark), None)?;
    } else {
        info!("→ Pushing bookmark and tags...");
        ctx.backend
            .git_push(Some(&config.release.bookmark), Some(&prepared.tag_name))?;
    }

    // Forge release.
    if !is_independent && config.release.create_release {
        info!("→ Creating forge release {}…", prepared.tag_name);
        ctx.forge.create_release(&prepared.tag_name)?;
    }

    info!("✓ Released {}", prepared.tag_name);
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
                            // Find which siblings also bumped and will have deps updated
                            let updated_deps: Vec<&str> = ws
                                .members
                                .iter()
                                .filter(|other| other.name != member.name)
                                .filter(|other| mb.get(&other.name).is_some_and(|(c, n)| c != n))
                                .map(|other| other.name.as_str())
                                .collect();

                            if updated_deps.is_empty() {
                                println!(
                                    "  - {} ({}) {} → {} (tag: {})",
                                    member.name, member.path, current, next, tag
                                );
                            } else {
                                println!(
                                    "  - {} ({}) {} → {} (tag: {}, deps updated: {})",
                                    member.name,
                                    member.path,
                                    current,
                                    next,
                                    tag,
                                    updated_deps.join(", ")
                                );
                            }
                        }
                        continue;
                    }
                    println!("  - {} ({})", member.name, member.path);
                }
            }
            Versioning::Unified => {
                for member in ordered {
                    println!(
                        "  - {} ({}) → {} (deps updated)",
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
