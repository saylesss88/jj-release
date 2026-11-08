//! Next version computation.

use std::path::Path;

use crate::pipeline::{self, ReleaseContext};
use crate::{ReleasePlan, config::Config, errors::Result, workspace};

/// Print the version(s) the next release would produce: the same plan
/// `jj-release` and `--dry-run` use.
///
/// # Errors
///
/// Returns an error if reading the version manifest, resolving the last
/// version tag, or fetching commit logs fails.
pub fn print_next_version(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> Result<()> {
    let Some(prepared) = pipeline::plan_release(ctx.backend, ctx.manifest, config, root)? else {
        eprintln!("Nothing to release (no trigger commit or no releasable commits).");
        return Ok(());
    };
    match &prepared.plan {
        ReleasePlan::Unified(plan) => println!("{}", plan.next_version),
        ReleasePlan::Independent(members) => {
            let Some(ws) = pipeline::independent_workspace(config) else {
                return Ok(());
            };
            for member in workspace::ordered_members(&ws.members)? {
                if let Some(plan) = members.get(&member.name).filter(|p| p.is_released()) {
                    println!("{} {}", member.name, plan.next);
                }
            }
        }
    }
    Ok(())
}
