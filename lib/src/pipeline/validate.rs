//! Pre-release validation checks for the release environment.
//!
//! Runs a suite of preflight checks: identity, forge CLI, manifest,
//! version tags, semver compatibility, crates.io sync, trigger commit,
//! and publish tokens, and reports pass/fail for each before any
//! mutations occur.

use std::{env, path::Path};

use crate::{
    Config, PublishBackend, ReleaseContext, Tag, commits, detect,
    errors::{ReleaseError, Result},
    manifest,
    pipeline::prepare,
    publish, registry,
};

type CheckResult = Result<String, String>;

/// Validates the release environment, checking backend identity, forge CLI availability, manifest readability, version tags, trigger commits, and tokens.
///
/// # Errors
///
/// Returns an error if any backend operations fail, the manifest cannot be read, or a required version tag is missing when `require_tag` is enabled.
pub fn validate(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> Result<()> {
    let mut passed = 0;
    let mut failed = 0;

    let mut run_check = |label: &str, result: CheckResult| match result {
        Ok(msg) => {
            println!("✓ {label}: {msg}");
            passed += 1;
        }
        Err(msg) => {
            println!("✗ {label}: {msg}");
            failed += 1;
        }
    };

    let tag = commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)
        .map_err(|e| e.to_string());

    run_check("jj identity", check_jj_identity(ctx));
    run_check("forge CLI", check_forge_cli(config));
    run_check("manifest", check_manifest(ctx, root));
    run_check("version tag", check_version_tag(&tag));
    run_check(
        "publish pre-flight (dry-run)",
        check_publish(ctx.publisher, root),
    );

    if config.publish.cargo && config.publish.semver_checks {
        run_check("cargo-semver-checks", check_semver(ctx, config, root));
    }
    if config.publish.cargo
        && let Some(result) = check_crates_io(ctx, root)
    {
        run_check("crates.io", result);
    }

    let since = prepare::resolve_since(ctx.backend, config).map_err(|e| e.to_string());

    run_check(
        "trigger commit",
        since
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|s| check_trigger(ctx, config, s)),
    );

    run_check(
        "version bump",
        since
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|s| check_bump(ctx, config, s)),
    );

    if config.publish.cargo {
        run_check("CARGO_REGISTRY_TOKEN", check_cargo_token());
    }

    println!("\n{passed} passed, {failed} failed");
    if failed > 0 {
        return Err(ReleaseError::Message(format!("{failed} check(s) failed")));
    }
    Ok(())
}

fn check_jj_identity(ctx: &ReleaseContext<'_>) -> CheckResult {
    ctx.backend
        .check_identity()
        .map(|()| "configured".to_owned())
        .map_err(|e| e.to_string())
}

fn check_forge_cli(config: &Config) -> CheckResult {
    match config.release.forge.as_str() {
        "github" if detect::tool_available("gh") => Ok("gh found".to_owned()),
        "github" => Err("gh not found. Install from https://cli.github.com".to_owned()),
        "gitlab" if detect::tool_available("glab") => Ok("glab found".to_owned()),
        "gitlab" => {
            Err("glab not found. Install from https://gitlab.com/gitlab-org/cli".to_owned())
        }
        _ => Ok("no forge CLI required".to_owned()),
    }
}

fn check_semver(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> CheckResult {
    if !detect::tool_available("cargo-semver-checks") {
        return Err("not found, install with: cargo install cargo-semver-checks".to_owned());
    }

    match publish::run_semver_checks(root) {
        Ok(false) => Ok("no breaking changes".to_owned()),
        Ok(true) => {
            let is_stable = ctx.manifest.read_version(root).is_ok_and(|v| v.major >= 1);
            let will_upgrade = is_stable || config.publish.semver_checks_upgrade_major;
            if will_upgrade {
                Err("breaking API changes detected, bump will be upgraded to Major".to_owned())
            } else {
                Ok("breaking API changes detected, skipping Major upgrade (pre-1.0)".to_owned())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

fn check_manifest(ctx: &ReleaseContext<'_>, root: &Path) -> CheckResult {
    ctx.manifest
        .read_version(root)
        .map(|v| format!("version {v}"))
        .map_err(|e| e.to_string())
}

fn check_version_tag(tag: &Result<Option<Tag>, String>) -> CheckResult {
    match tag {
        Ok(Some(t)) => Ok(format!("found {}", t.name)),
        Ok(None) => Ok("none (first release mode)".to_owned()),
        Err(e) => Err(e.clone()),
    }
}

fn check_crates_io(ctx: &ReleaseContext<'_>, root: &Path) -> Option<CheckResult> {
    let cargo_toml = root.join("Cargo.toml");
    let name = manifest::read_name(&cargo_toml).ok()?;
    let version = ctx.manifest.read_version(root).ok()?;

    let result = registry::latest_version_on_crates_io(&name)
        .map_err(|e| e.to_string())
        .map(|latest| match latest {
            Some(published) if published == version => {
                format!("v{version} published, in sync with manifest")
            }
            Some(published) => format!("v{published} published, manifest is v{version}"),
            None => "not yet published".to_owned(),
        });

    Some(result)
}

fn check_trigger(ctx: &ReleaseContext<'_>, config: &Config, since: &str) -> CheckResult {
    commits::find_trigger(ctx.backend, &config.release.trigger, since)
        .map_err(|e| e.to_string())
        .and_then(|t| t.ok_or_else(|| format!("no {:?} commit found", config.release.trigger)))
        .map(|_| "found".to_owned())
}

fn check_cargo_token() -> CheckResult {
    let has_env_token = env::var("CARGO_REGISTRY_TOKEN").is_ok();
    let has_credentials =
        dirs::home_dir().is_some_and(|h| h.join(".cargo/credentials.toml").exists());

    if has_env_token || has_credentials {
        Ok("configured".to_owned())
    } else {
        Err("not set and no ~/.cargo/credentials.toml found, needed for cargo publish".to_owned())
    }
}

pub(super) fn check_publish(publisher: &dyn PublishBackend, root: &Path) -> CheckResult {
    publisher
        .check(root)
        .map(|()| "dry-run passed".to_owned())
        .map_err(|e| e.to_string())
}

fn check_bump(
    ctx: &crate::pipeline::ReleaseContext<'_>,
    config: &crate::config::Config,
    since: &str,
) -> Result<String, String> {
    // If it's a first release, we bypass the bump check completely!
    if since == "root()" {
        return Ok("first release mode (version frozen)".to_owned());
    }

    let commits = ctx
        .backend
        .log_commits(&format!("{since}..@"))
        .map_err(|e| e.to_string())?;

    let bump = crate::commits::resolve_bump(config.bump.force.as_ref(), &commits)
        .map_err(|e| e.to_string())?;

    if bump == crate::commits::BumpKind::None {
        Err("no releasable commits (feat/fix/BREAKING) found since last tag".to_owned())
    } else {
        Ok("releasable commits found".to_owned())
    }
}
