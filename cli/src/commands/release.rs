use std::{
    io::{self, IsTerminal, Write},
    path::Path,
};

use jiff::Zoned;
use semver::Version;

use jj_release_core::{
    bump,
    config::{Config, Versioning},
    errors::Result,
    manifest,
    pipeline::{self, PreparedRelease, ReleaseContext},
    workspace,
};

macro_rules! info {
    ($quiet:expr, $($t:tt)*) => { if !$quiet { println!($($t)*); } };
}

pub fn release_pipeline(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    let Some(prepared) = pipeline::prepare_release(ctx.backend, ctx.manifest, config, root)? else {
        print_nothing_to_release(config, quiet);
        return Ok(());
    };

    let is_independent = config
        .workspace
        .as_ref()
        .is_some_and(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent));

    print_versions(&prepared, is_independent, quiet);

    if dry_run {
        return print_dry_run(&prepared, config);
    }

    // Run pre-flight checks on all targets before touching anything
    info!(quiet, " → Running pre-flight checks (dry-run)...");
    pipeline::run_preflight_checks(ctx, root, config, is_independent)?;

    // Warn if publishing is disabled, prompt interactively, skip in CI.
    if !config.publish.cargo && confirm_skip_cargo_publish()? {
        println!("Aborted.");
        return Ok(());
    }

    if !is_independent {
        create_release_commit(ctx, config, root, &prepared, quiet)?;
    }

    // Publish to registry
    pipeline::run_publish(ctx, config, root, &prepared)?;

    // Push to remote
    if is_independent {
        move_bookmark_and_export(ctx, config, quiet)?;
        info!(quiet, "→ Pushing bookmark…");
        ctx.backend.git_push(Some(&config.release.bookmark), None)?;
    } else {
        info!(quiet, "→ Pushing bookmark and tags...");
        ctx.backend
            .git_push(Some(&config.release.bookmark), Some(&prepared.tag_name))?;

        if config.release.create_release {
            info!(quiet, "→ Creating forge release {}…", prepared.tag_name);
            ctx.forge.create_release(&prepared.tag_name)?;
        }

        update_distro_packages(config, root, &prepared.next_version, quiet);
    }

    info!(quiet, "✓ Released {}", prepared.tag_name);
    Ok(())
}

fn print_nothing_to_release(config: &Config, quiet: bool) {
    info!(
        quiet,
        "No trigger commit found or no releasable commits. Nothing to release."
    );
    info!(quiet, "");
    info!(
        quiet,
        "hint: To trigger a release, create a commit with the exact message:"
    );
    info!(quiet, "      jj new -m {:?}", config.release.trigger);
    info!(
        quiet,
        "      (Run `jj-release --help` for full usage details)"
    );
}

fn print_versions(prepared: &PreparedRelease, is_independent: bool, quiet: bool) {
    info!(quiet, "  Current version: {}", prepared.current_version);
    if !is_independent && prepared.baseline_version != prepared.current_version {
        info!(quiet, "  crates.io version: {}", prepared.baseline_version);
        info!(quiet, "  (using crates.io version as bump baseline)");
    }
}

/// `publish.cargo = false`: ask before continuing in a terminal, just warn in CI.
fn confirm_skip_cargo_publish() -> Result<bool> {
    if !io::stdin().is_terminal() {
        eprintln!("warning: publish.cargo = false, skipping crates.io publish.");
        return Ok(true);
    }
    eprint!(
        "warning: publish.cargo = false. This will still bump versions, tag, and push, \
         but will NOT publish to crates.io. Continue? [y/N] "
    );
    let _ = io::stderr().flush();
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Changelog, version bumps, release commit, tag, bookmark and git export.
fn create_release_commit(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    prepared: &PreparedRelease,
    quiet: bool,
) -> Result<()> {
    if config.changelog.enabled {
        info!(quiet, "→ Writing changelog…");
        pipeline::update_changelog(root, config, prepared)?;
    }

    info!(quiet, "→ Bumping version to {}…", prepared.next_version);
    ctx.manifest.write_version(root, &prepared.next_version)?;
    pipeline::update_workspace_dependencies(root, config, &prepared.next_version)?;
    apply_file_bumps(config, root, &prepared.next_version.to_string(), quiet)?;

    let message = format!("chore: release {}", prepared.tag_name);
    info!(quiet, "→ Creating commit {message:?}…");
    ctx.backend.new_commit(&message)?;

    info!(quiet, "→ Creating tag {}…", prepared.tag_name);
    ctx.backend.create_tag(&prepared.tag_name, "@")?;

    move_bookmark_and_export(ctx, config, quiet)
}

/// User-configured replacements (README, constants) and flake.nix.
fn apply_file_bumps(config: &Config, root: &Path, version: &str, quiet: bool) -> Result<()> {
    if !config.replacements.is_empty() {
        info!(quiet, "→ Applying text replacements…");
        let crate_name = manifest::read_name(&root.join("Cargo.toml"))
            .unwrap_or_else(|_| "unknown_crate".to_owned());
        let date = Zoned::now().strftime("%Y-%m-%d").to_string();
        bump::apply_replacements(&config.replacements, &crate_name, version, &date)?;
    }
    bump::bump_flake_nix(root, version)
}

fn move_bookmark_and_export(ctx: &ReleaseContext<'_>, config: &Config, quiet: bool) -> Result<()> {
    info!(
        quiet,
        "→ Moving bookmark {:?} to @…", config.release.bookmark
    );
    ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
    info!(quiet, "→ Exporting to git…");
    ctx.backend.git_export()
}

/// Runs after the release is public, so failures warn instead of failing the
/// run: the crates.io publish, tag and forge release can't be redone.
#[cfg_attr(not(feature = "publish-copr"), allow(unused_variables))]
fn update_distro_packages(config: &Config, root: &Path, version: &Version, quiet: bool) {
    #[cfg(feature = "publish-aur")]
    if let Some(aur) = &config.aur {
        info!(quiet, "→ Updating AUR package {}…", aur.package);
        if let Err(e) = jj_release_core::distro::aur::publish(aur, version, false) {
            eprintln!("warning: AUR update failed: {e}");
        }
    }

    #[cfg(feature = "publish-copr")]
    if let Some(copr) = &config.copr {
        info!(quiet, "→ Submitting to COPR project {}…", copr.project);
        if let Err(e) = jj_release_core::distro::copr::publish(copr, root, version, false) {
            eprintln!("warning: COPR submission failed: {e}");
        }
    }
}

fn print_dry_run(prepared: &PreparedRelease, config: &Config) -> Result<()> {
    #[cfg(feature = "publish-aur")]
    if let Some(aur_cfg) = &config.aur {
        println!(
            "  - Push to AUR package '{}' (bumping to {})",
            aur_cfg.package, prepared.next_version
        );
    }

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

    if !config.publish.cargo {
        println!("[dry-run] Publishing: disabled (publish.cargo = false)");
        return Ok(());
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
