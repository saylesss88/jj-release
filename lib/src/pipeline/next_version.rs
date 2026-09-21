//! Next version computation.

use std::path::Path;

use crate::commits;
use crate::config::Config;
use crate::errors::Result;
use crate::pipeline::{ReleaseContext, prepare};

/// Computes and prints the next release version to stdout based on
/// conventional commits since the last version tag.
///
/// # Errors
///
/// Returns an error if reading the version manifest, resolving the last
/// version tag, or fetching commit logs fails.
pub fn print_next_version(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> Result<()> {
    let current = ctx.manifest.read_version(root)?;
    let since = prepare::resolve_since(ctx.backend, config)?;
    let commits = ctx.backend.log_commits(&format!("{since}..@"))?;
    let bump = commits::resolve_bump(config.bump.force.as_ref(), &commits)?;
    let next = commits::apply_bump(&current, bump);
    println!("{next}");
    Ok(())
}
