use std::{
    collections::HashMap,
    io::{self, IsTerminal, Write},
    path::Path,
};

use jiff::Zoned;

use jj_release_core::{
    MemberPlan, ReleasePlan, UnifiedPlan, bump,
    config::{Config, WorkspaceMember},
    distro,
    errors::{ReleaseError, Result},
    manifest,
    pipeline::{self, PreparedRelease, ReleaseContext},
    workspace,
};

macro_rules! info {
    ($quiet:expr, $($t:tt)*) => { if !$quiet { println!($($t)*); } };
}

/// How far a release got before failing: decides how to recover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reached {
    /// Only local changes: restoring the start operation undoes everything.
    Local,
    /// Publishing to crates.io, nothing pushed yet. Restoring is still safe,
    /// and a re-run skips crates that are already published.
    Publishing,
    /// Something was pushed: restoring would only undo the local side.
    Pushed,
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

    print_plan(&prepared, config, dry_run, quiet)?;

    if dry_run {
        return print_dry_run(&prepared, config);
    }

    // Run pre-flight checks on all targets before touching anything
    info!(quiet, " → Running pre-flight checks (dry-run)...");
    pipeline::run_preflight_checks(ctx, root, config, prepared.is_independent())?;

    // Warn if publishing is disabled, prompt interactively, skip in CI.
    if !config.publish.cargo && confirm_skip_cargo_publish()? {
        println!("Aborted.");
        return Ok(());
    }

    // Everything from here changes the repo. Record where it started, so a
    // failure can say how to undo it.
    let start_op = ctx.backend.current_operation()?;
    let mut reached = Reached::Local;

    let result = match &prepared.plan {
        ReleasePlan::Unified(plan) => {
            release_unified(ctx, config, root, &prepared, plan, quiet, &mut reached)
        }
        ReleasePlan::Independent(_) => {
            release_independent(ctx, config, root, &prepared, quiet, &mut reached)
        }
    };
    result.map_err(|e| with_recovery_hint(&e, &start_op, reached))
}

fn release_unified(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    prepared: &PreparedRelease,
    plan: &UnifiedPlan,
    quiet: bool,
    reached: &mut Reached,
) -> Result<()> {
    create_release_commit(ctx, config, root, prepared, plan, quiet)?;

    *reached = Reached::Publishing;
    pipeline::run_publish(ctx, config, root, prepared)?;

    *reached = Reached::Pushed;

    info!(quiet, "→ Pushing bookmark and tags...");
    ctx.backend
        .git_push(Some(&config.release.bookmark), Some(&plan.tag_name))?;

    if config.release.create_release {
        info!(quiet, "→ Creating forge release {}…", plan.tag_name);
        ctx.forge.create_release(&plan.tag_name)?;
    }

    pipeline::run_distro(config, root, prepared)?;

    info!(quiet, "✓ Released {}", plan.tag_name);
    Ok(())
}

fn release_independent(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    prepared: &PreparedRelease,
    quiet: bool,
    reached: &mut Reached,
) -> Result<()> {
    // Written before the per-member commits, so the changelog files land in
    // the first member's release commit.
    if config.changelog.enabled {
        info!(quiet, "→ Writing changelogs…");
        pipeline::update_changelog(ctx, root, config, prepared)?;
    }

    // Each member's tag is pushed as it's published.
    *reached = Reached::Pushed;
    // Per member: bump, commit, tag, push tag, publish.
    pipeline::run_publish(ctx, config, root, prepared)?;

    move_bookmark_and_export(ctx, config, quiet)?;
    info!(quiet, "→ Pushing bookmark…");
    ctx.backend.git_push(Some(&config.release.bookmark), None)?;

    let released = prepared
        .members()
        .map_or(0, |m| m.values().filter(|p| p.is_released()).count());
    info!(quiet, "✓ Released {released} member(s)");
    Ok(())
}

fn print_plan(
    prepared: &PreparedRelease,
    config: &Config,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    match &prepared.plan {
        ReleasePlan::Unified(plan) => {
            info!(quiet, "  Current version: {}", plan.current_version);
            if plan.baseline_version != plan.current_version {
                info!(quiet, "  Last released version: {}", plan.baseline_version);
                info!(quiet, "  (using it as the bump baseline)");
            }
        }
        ReleasePlan::Independent(_) if dry_run => {}
        ReleasePlan::Independent(members) => {
            let Some(ws) = pipeline::independent_workspace(config) else {
                return Ok(());
            };
            for member in workspace::ordered_members(&ws.members)? {
                let Some(plan) = members.get(&member.name) else {
                    continue;
                };
                if plan.first_release {
                    info!(quiet, "  {}: {} (first release)", member.name, plan.next);
                } else if plan.is_released() {
                    info!(quiet, "  {}: {} → {}", member.name, plan.current, plan.next);
                }
            }
        }
    }
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
    plan: &UnifiedPlan,
    quiet: bool,
) -> Result<()> {
    if config.changelog.enabled {
        info!(quiet, "→ Writing changelog…");
        pipeline::update_changelog(ctx, root, config, prepared)?;
    }

    info!(quiet, "→ Bumping version to {}…", plan.next_version);
    ctx.manifest.write_version(root, &plan.next_version)?;
    pipeline::update_workspace_dependencies(root, config, &plan.next_version)?;
    apply_file_bumps(config, root, &plan.next_version.to_string(), quiet)?;

    let message = format!("chore: release {}", plan.tag_name);
    info!(quiet, "→ Creating commit {message:?}…");
    ctx.backend.new_commit(&message)?;

    info!(quiet, "→ Creating tag {}…", plan.tag_name);
    ctx.backend.create_tag(&plan.tag_name, "@")?;

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

fn print_dry_run(prepared: &PreparedRelease, config: &Config) -> Result<()> {
    match &prepared.plan {
        ReleasePlan::Unified(plan) => {
            println!(
                "[dry-run] Would release {} as {}",
                plan.next_version, plan.tag_name
            );
            print_publish_plan(prepared, config)?;
            print_distro_plan(plan, config);
        }
        ReleasePlan::Independent(_) => {
            println!("[dry-run] Independent workspace release:");
            print_publish_plan(prepared, config)?;
        }
    }
    Ok(())
}

fn print_distro_plan(plan: &UnifiedPlan, config: &Config) {
    let plan = distro::describe(config, &plan.next_version);
    if plan.is_empty() {
        return;
    }
    if !config.publish.cargo {
        println!("[dry-run] Distro packages: skipped (publish.cargo = false)");
        return;
    }
    println!("[dry-run] Would update distro packages:");
    for line in plan {
        println!("  - {line}");
    }
}

fn print_publish_plan(prepared: &PreparedRelease, config: &Config) -> Result<()> {
    let Some(ws) = config.workspace.as_ref().filter(|ws| ws.enabled) else {
        if config.publish.cargo {
            println!("[dry-run] Would publish from root");
        } else {
            println!("[dry-run] Publishing: disabled (publish.cargo = false)");
        }
        return Ok(());
    };

    if config.publish.cargo {
        println!("[dry-run] Would publish in order:");
    } else {
        println!("[dry-run] Would release in order (publishing disabled):");
    }
    for member in workspace::ordered_members(&ws.members)? {
        match &prepared.plan {
            ReleasePlan::Independent(members) => print_independent_member(member, members, config),
            ReleasePlan::Unified(plan) => println!(
                "  - {} ({}) → {} (deps updated)",
                member.name, member.path, plan.next_version
            ),
        }
    }
    Ok(())
}

fn print_independent_member(
    member: &WorkspaceMember,
    members: &HashMap<String, MemberPlan>,
    config: &Config,
) {
    let Some(plan) = members.get(&member.name) else {
        println!("  - {} ({})", member.name, member.path);
        return;
    };

    if !plan.is_released() {
        println!(
            "  - {} ({}) {} (no changes, skipping)",
            member.name, member.path, plan.current
        );
        return;
    }

    let tag = workspace::member_tag_name(member, &plan.next, &config.release.tag_prefix);

    // Dependencies of this member that are also released in this run,
    // so their version requirements in its Cargo.toml get updated.
    let updated_deps: Vec<&str> = member
        .depends_on
        .iter()
        .filter(|dep| members.get(*dep).is_some_and(MemberPlan::is_released))
        .map(String::as_str)
        .collect();

    let deps = if updated_deps.is_empty() {
        String::new()
    } else {
        let labelled: Vec<String> = updated_deps
            .iter()
            .map(|dep| {
                if plan.updated_deps.iter().any(|d| d == dep) {
                    format!("{dep} (breaking)")
                } else {
                    (*dep).to_owned()
                }
            })
            .collect();
        format!(", deps updated: {}", labelled.join(", "))
    };

    if plan.first_release {
        println!(
            "  - {} ({}) {} (first release, tag: {tag}{deps})",
            member.name, member.path, plan.next
        );
    } else if plan.propagated {
        println!(
            "  - {} ({}) {} → {} (dependency update, tag: {tag}{deps})",
            member.name, member.path, plan.current, plan.next
        );
    } else {
        println!(
            "  - {} ({}) {} → {} (tag: {tag}{deps})",
            member.name, member.path, plan.current, plan.next
        );
    }
}

/// `error`, followed by how to recover from where the release stopped.
fn with_recovery_hint(error: &ReleaseError, start_op: &str, reached: Reached) -> ReleaseError {
    let hint = match reached {
        Reached::Local | Reached::Publishing => {
            let mut hint = format!(
                "Nothing was pushed. To undo this release's local changes and try again:\n  \
             jj op restore {start_op}"
            );
            if reached == Reached::Publishing {
                hint.push_str(
                    "\nCrates already on crates.io are skipped when you run jj-release again.",
                );
            }
            hint
        }
        Reached::Pushed => "The release was already pushed, so don't restore: that would only \
                        undo your local side. Finish the remaining steps by hand."
            .to_owned(),
    };
    ReleaseError::Message(format!("{error}\n\n{hint}"))
}
