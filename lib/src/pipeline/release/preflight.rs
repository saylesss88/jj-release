use std::path::Path;

use crate::{
    config::Config, distro, errors::ReleaseError, errors::Result, pipeline::ReleaseContext,
    workspace,
};

/// Executes pre-flight validation checks before mutating the repository.
///
/// For independent workspaces, members are checked in topological dependency order.
/// Otherwise, a single check is executed at the project root.
///
/// # Errors
///
/// Returns an error if a circular dependency is detected among workspace members,
/// or if the publisher's check mechanism (e.g., `cargo publish --dry-run`)
pub fn run_preflight_checks(
    ctx: &ReleaseContext<'_>,
    root: &Path,
    config: &Config,
    is_independent: bool,
) -> Result<()> {
    if is_independent {
        if let Some(ws) = &config.workspace {
            for member in workspace::ordered_members(&ws.members)? {
                ctx.publisher.check(&root.join(&member.path))?;
            }
        }
        return Ok(());
    }

    if config.publish.cargo {
        let mut fatal = Vec::new();
        for (label, required, result) in distro::preflight(config, root) {
            match result {
                Ok(_) => {}
                Err(e) if required => fatal.push(format!("  {label}: {e}")),
                Err(e) => eprintln!("⚠ {label} (not required): {e}"),
            }
        }
        if !fatal.is_empty() {
            return Err(ReleaseError::Message(format!(
                "distro pre-flight checks failed:\n{}",
                fatal.join("\n")
            )));
        }
    }
    ctx.publisher.check(root)
}
