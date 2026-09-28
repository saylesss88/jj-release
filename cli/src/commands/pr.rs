use std::path::Path;

use jj_release_core::{
    changelog,
    config::Config,
    errors::Result,
    forge::PrRequest,
    pipeline::{self, ReleaseContext},
};

macro_rules! info {
    ($quiet:expr, $($t:tt)*) => { if !$quiet { println!($($t)*); } };
}

pub fn release_pr(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    quiet: bool,
) -> Result<()> {
    let Some(prepared) = pipeline::prepare_release(ctx.backend, ctx.manifest, config, root)? else {
        info!(
            quiet,
            "No trigger commit found or no releasable commits. Nothing to release."
        );
        info!(quiet, "");
        info!(
            quiet,
            "hint: To trigger a PR, create a commit with the exact message:"
        );
        info!(quiet, "      jj new -m {:?}", config.release.trigger);
        info!(
            quiet,
            "      (Run `jj-release --help` for full usage details)"
        );
        return Ok(());
    };

    info!(quiet, "  Found trigger commit.");
    info!(quiet, "  Current version: {}", prepared.current_version);
    info!(
        quiet,
        "  Bump: {:?} → {}  (tag: {})", prepared.bump, prepared.next_version, prepared.tag_name
    );

    // Generate changelog preview for PR body, no file write.
    let body = changelog::render_changelog_section(
        &prepared.commits,
        &prepared.next_version,
        &config.changelog,
    );

    // Push to a release bookmark so PR has something to merge into main.
    let pr_bookmark = format!("release/{}", prepared.tag_name);
    info!(quiet, "→ Creating bookmark {pr_bookmark}…");
    ctx.backend.set_bookmark(&pr_bookmark, "@")?;
    info!(quiet, "→ Exporting to git…");
    ctx.backend.git_export()?;
    info!(quiet, "→ Pushing {pr_bookmark}…");
    ctx.backend.git_push(Some(&pr_bookmark), None)?;
    info!(quiet, "→ Opening PR…");
    ctx.forge.create_pr(&PrRequest {
        tag: &prepared.tag_name,
        head: &pr_bookmark,
        base: &config.release.bookmark,
        body: &body,
    })?;

    info!(quiet, "✓ PR opened for {}", prepared.tag_name);
    Ok(())
}
