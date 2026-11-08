use std::path::Path;

use crate::{
    config::Config, distro, errors::ReleaseError, errors::Result, pipeline::ReleaseContext,
    registry, workspace,
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
    if config.publish.cargo {
        check_publish_token()?;
    }

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

/// Stop before anything is committed if crates.io rejects the publish token.
/// Only a definite rejection fails: a token that can't be checked (say, one
/// held by a credential provider) gets a warning instead.
fn check_publish_token() -> Result<()> {
    match registry::token_status() {
        Ok(registry::TokenStatus::Rejected) => Err(ReleaseError::Message(
            "crates.io rejected your publish token (expired or revoked?). \
             Create a new one at https://crates.io/settings/tokens and run `cargo login`."
                .to_owned(),
        )),
        Ok(registry::TokenStatus::Accepted) => Ok(()),
        Ok(registry::TokenStatus::Unknown(why)) => {
            eprintln!("⚠ couldn't verify the crates.io token: {why}");
            Ok(())
        }
        Err(e) => {
            eprintln!("⚠ couldn't verify the crates.io token: {e}");
            Ok(())
        }
    }
}
