use std::fmt::Write;
use std::path::Path;

use jj_release_core::{
    ReleaseContext, ReleasePlan, config::Config, errors::ReleaseError, errors::Result,
    forge::PrRequest, pipeline, workspace,
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

    // (bookmark, PR title tag, PR body)
    let (pr_bookmark, title, body) = match &prepared.plan {
        ReleasePlan::Unified(plan) => {
            info!(quiet, "  Current version: {}", plan.current_version);
            info!(
                quiet,
                "  Bump: {:?} → {}  (tag: {})", plan.bump, plan.next_version, plan.tag_name
            );
            let body = jj_release_changelog::render_changelog_section(
                &prepared.commits,
                Some(&plan.next_version),
                &config.changelog,
                None,
            );
            (
                format!("release/{}", plan.tag_name),
                plan.tag_name.clone(),
                body,
            )
        }
        ReleasePlan::Independent(members) => {
            let ws = pipeline::independent_workspace(config).ok_or_else(|| {
                ReleaseError::Message("independent plan without an independent workspace".into())
            })?;

            // One line per released member, in publish order.
            let mut tags = Vec::new();
            let mut summary = String::from("## Releases\n\n");
            for member in workspace::ordered_members(&ws.members)? {
                let Some(plan) = members.get(&member.name).filter(|p| p.is_released()) else {
                    continue;
                };
                let tag =
                    workspace::member_tag_name(member, &plan.next, &config.release.tag_prefix);
                info!(
                    quiet,
                    "  {}: {} → {}  (tag: {tag})", member.name, plan.current, plan.next
                );
                let _ = writeln!(
                    summary,
                    "- {}: {} → {}\n",
                    member.name, plan.current, plan.next
                );
                tags.push(tag);
            }
            if tags.is_empty() {
                info!(
                    quiet,
                    "No members have releasable changes. Nothing to release."
                );
                return Ok(());
            }

            let notes = jj_release_changelog::render_changelog_section(
                &prepared.commits,
                None,
                &config.changelog,
                None,
            );
            let date = jiff::Zoned::now().date();
            (
                format!("release/{date}"),
                tags.join(", "),
                format!("{summary}\n{notes}"),
            )
        }
    };

    info!(quiet, "→ Creating bookmark {pr_bookmark}…");
    ctx.backend.set_bookmark(&pr_bookmark, "@")?;
    info!(quiet, "→ Exporting to git…");
    ctx.backend.git_export()?;
    info!(quiet, "→ Pushing {pr_bookmark}…");
    ctx.backend.git_push(Some(&pr_bookmark), None)?;
    info!(quiet, "→ Opening PR…");
    ctx.forge.create_pr(&PrRequest {
        tag: &title,
        head: &pr_bookmark,
        base: &config.release.bookmark,
        body: &body,
    })?;

    info!(quiet, "✓ PR opened for {title}");
    Ok(())
}
