use std::path::Path;

use anyhow::Result;
use jj_release::changelog;
use jj_release::config::Config;
use jj_release::pipeline::{self, ReleaseContext};

pub fn release_pr(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let Some(prepared) = pipeline::prepare_release(ctx.backend, ctx.manifest, config, root)? else {
        info!("No trigger commit found or no releasable commits. Nothing to release.");
        return Ok(());
    };

    info!("  Found trigger commit.");
    info!("  Current version: {}", prepared.current_version);
    info!(
        "  Bump: {:?} → {}  (tag: {})",
        prepared.bump, prepared.next_version, prepared.tag_name
    );

    // Generate changelog preview for PR body — no file write.
    let body = changelog::render_changelog_section(&prepared.commits, &prepared.next_version);

    // Push to a release bookmark so PR has something to merge into main.
    let pr_bookmark = format!("release/{}", prepared.tag_name);
    info!("→ Creating bookmark {pr_bookmark}…");
    ctx.backend.set_bookmark(&pr_bookmark, "@")?;
    info!("→ Exporting to git…");
    ctx.backend.git_export()?;
    info!("→ Pushing {pr_bookmark}…");
    ctx.backend.git_push(&pr_bookmark, None)?;
    info!("→ Opening PR…");
    ctx.forge.create_pr(
        &prepared.tag_name,
        &pr_bookmark,
        &config.release.bookmark,
        &body,
    )?;

    info!("✓ PR opened for {}", prepared.tag_name);
    Ok(())
}
