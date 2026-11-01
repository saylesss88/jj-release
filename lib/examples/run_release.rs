//! Demonstrates how to run the full release pipeline programmatically.
//!
//! This example shows how to use `jj_release_core` as a library to orchestrate
//! a complete release without invoking the CLI.
//!
//! # Warning
//!
//! **This example makes real changes**: it writes files, creates commits,
//! creates tags, and pushes to the remote. Run it only in a repo where you
//! intend to make a release. Use `jj-release --dry-run` to preview first.
//!
//! Run with: `cargo run --example run_release -- /path/to/repo`

use std::{env, path::PathBuf};

use jj_release_core::{
    ReleaseContext, ReleasePlan, config, errors::Result, forge::NoForge, jj::ShellBackend,
    manifest, pipeline, publish::NoPublish,
};

fn main() -> Result<()> {
    let root = env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let root = root.as_path();

    let backend = ShellBackend::new(root)?;
    let manifest = manifest::detect_manifest(root);
    let forge = NoForge;
    let publisher = NoPublish;
    let ctx = ReleaseContext::new(&backend, manifest.as_ref(), &forge, &publisher);
    let mut config = config::load(root)?;
    // Set to true to publish on crates.io after tagging
    config.publish.cargo = false;

    let Some(prepared) = pipeline::prepare_release(&backend, manifest.as_ref(), &config, root)?
    else {
        println!("No trigger commit found or no releasable commits.");
        return Ok(());
    };

    let ReleasePlan::Unified(plan) = &prepared.plan else {
        println!(
            "Independent workspace detected; use the `jj-release` CLI for per-member releases."
        );
        return Ok(());
    };

    println!("  Current version: {}", plan.current_version);
    println!("  Next version:    {}", plan.next_version);
    println!("  Tag:             {}", plan.tag_name);

    // Preflight checks
    println!("→ Running preflight checks...");
    pipeline::run_preflight_checks(&ctx, root, &config, false)?;

    // Write changelog
    if config.changelog.enabled {
        println!("→ Writing changelog...");
        pipeline::update_changelog(&ctx, root, &config, &prepared)?;
    }

    // Bump version
    println!("→ Bumping version to {}...", plan.next_version);
    ctx.manifest.write_version(root, &plan.next_version)?;
    pipeline::update_workspace_dependencies(root, &config, &plan.next_version)?;

    // Commit and tag
    ctx.backend
        .new_commit(&format!("chore: release {}", plan.tag_name))?;
    ctx.backend.create_tag(&plan.tag_name, "@")?;
    ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
    ctx.backend.git_export()?;

    // Publish
    println!("→ Publishing...");
    pipeline::run_publish(&ctx, &config, root, &prepared)?;

    // Push: this is irreversible
    println!("→ Pushing...");
    ctx.backend
        .git_push(Some(&config.release.bookmark), Some(&plan.tag_name))?;

    // Forge release
    if config.release.create_release {
        println!("→ Creating forge release...");
        ctx.forge.create_release(&plan.tag_name)?;
    }

    println!("✓ Released {}", plan.tag_name);
    Ok(())
}
