//! Pre-release validation checks for the release environment.
//!
//! Runs a suite of preflight checks: identity, forge CLI, manifest,
//! version tags, semver compatibility, crates.io sync, trigger commit,
//! and publish tokens, and reports pass/fail for each before any
//! mutations occur.

use std::{env, path::Path};

use crate::{
    Config, PublishBackend, ReleaseContext, WorkspaceConfig,
    commits::{self, BumpKind},
    detect, distro,
    errors::{ReleaseError, Result},
    manifest,
    pipeline::{self, prepare},
    publish, registry, workspace,
};

type CheckResult = Result<String, String>;

// -- Validate --

/// Encapsulates the tracking and reporting of validation checks.
#[derive(Default)]
struct Validator {
    passed: usize,
    failed: usize,
}

impl Validator {
    /// Executes a check, logs the result, and updates the internal counters.
    fn check(&mut self, label: &str, result: Result<String, String>) {
        match result {
            Ok(msg) => {
                println!("✓ {label}: {msg}");
                self.passed += 1;
            }
            Err(msg) => {
                println!("✗ {label}: {msg}");
                self.failed += 1;
            }
        }
    }

    /// Evaluates the final results, printing a summary and returning an error if any checks failed.
    fn finish(self) -> Result<()> {
        println!("\n{} passed, {} failed", self.passed, self.failed);
        if self.failed > 0 {
            Err(ReleaseError::Message(format!(
                "{} check(s) failed",
                self.failed
            )))
        } else {
            Ok(())
        }
    }
}

/// Validates the release environment, checking backend identity, forge CLI availability, manifest
/// readability, version tags, trigger commits, and tokens.
///
/// # Errors
///
/// Returns an error if any backend operations fail, or ifthe manifest cannot be read.
pub fn validate(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> Result<()> {
    let mut v = Validator::default();
    let independent = pipeline::independent_workspace(config);

    validate_environment(&mut v, ctx, config);
    validate_manifests(&mut v, ctx, config, root, independent);
    validate_distro(&mut v, config, root, independent);
    validate_publishing(&mut v, ctx, config, root, independent);
    validate_versioning(&mut v, ctx, config, root, independent);

    v.finish()
}

fn validate_environment(v: &mut Validator, ctx: &ReleaseContext<'_>, config: &Config) {
    v.check("jj identity", check_jj_identity(ctx));
    v.check("forge CLI", check_forge_cli(config));

    if config.publish.cargo {
        v.check("CARGO_REGISTRY_TOKEN", check_cargo_token());
    }
}

fn validate_manifests(
    v: &mut Validator,
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    independent: Option<&WorkspaceConfig>,
) {
    let manifest_res = independent.map_or_else(
        || check_manifest(ctx, root),
        |ws| check_member_manifests(ws, root),
    );
    v.check("manifest", manifest_res);
    v.check("workspace members", check_workspace_members(config, root));
}

fn validate_distro(
    v: &mut Validator,
    config: &Config,
    root: &Path,
    independent: Option<&WorkspaceConfig>,
) {
    if !distro::any_configured(config) {
        return;
    }

    if !config.publish.cargo {
        v.check(
            "distro packages",
            Ok("skipped (publish.cargo = false)".to_owned()),
        );
    } else if independent.is_some() {
        v.check(
            "distro packages",
            Ok("skipped (independent versioning)".to_owned()),
        );
    } else {
        for (label, required, result) in distro::preflight(config, root) {
            let res = match result {
                Err(e) if !required => Ok(format!("warning, not required: {e}")),
                other => other,
            };
            v.check(label, res);
        }
    }
}

fn validate_publishing(
    v: &mut Validator,
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    independent: Option<&WorkspaceConfig>,
) {
    if !config.publish.cargo {
        v.check(
            "publish pre-flight (dry-run)",
            Ok("skipped (publish.cargo = false)".to_owned()),
        );
        return;
    }

    v.check(
        "publish pre-flight (dry-run)",
        check_publish(ctx.publisher, root),
    );

    if config.publish.semver_checks {
        let res = if independent.is_some() {
            Ok("skipped (not used for independent versioning)".to_owned())
        } else {
            check_semver(ctx, config, root)
        };
        v.check("cargo-semver-checks", res);
    }

    let crates_io_res = independent.map_or_else(
        || check_crates_io(ctx, root),
        |ws| Some(check_members_on_crates_io(ws, root)),
    );

    if let Some(res) = crates_io_res {
        v.check("crates.io", res);
    }
}

fn validate_versioning(
    v: &mut Validator,
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    independent: Option<&WorkspaceConfig>,
) {
    let tag_res = independent.map_or_else(
        || check_version_tag(ctx, config),
        |ws| check_member_tags(ctx, config, ws),
    );
    v.check("version tag", tag_res);

    let since_res = prepare::release_since(ctx.backend, config).map_err(|e| e.to_string());

    // Cleaned up the `.and_then(Clone::clone)` clutter using standard pattern matching
    let trigger_res = match &since_res {
        Ok(since) => check_trigger(ctx, config, since),
        Err(e) => Err(e.clone()),
    };
    v.check("trigger commit", trigger_res);

    let bump_res = match independent {
        Some(_) => check_member_bumps(ctx, config, root),
        None => match &since_res {
            Ok(since) => check_bump(ctx, config, since),
            Err(e) => Err(e.clone()),
        },
    };
    v.check("version bump", bump_res);
}

// -- Checks --

fn check_workspace_members(config: &Config, root: &Path) -> CheckResult {
    let Some(ws) = config.workspace.as_ref().filter(|w| w.enabled) else {
        return Ok("not a workspace".to_owned());
    };
    let Ok(Some(detected)) = workspace::detect_workspace(root) else {
        return Err(
            "workspace enabled in release.toml but no [workspace] in Cargo.toml".to_owned(),
        );
    };

    let configured: Vec<&str> = ws.members.iter().map(|m| m.name.as_str()).collect();
    let missing: Vec<&str> = detected
        .members
        .iter()
        .map(|m| m.name.as_str())
        .filter(|name| !configured.contains(name))
        .collect();

    if missing.is_empty() {
        Ok(format!("{} members match Cargo.toml", configured.len()))
    } else {
        Err(format!(
            "in Cargo.toml but not release.toml: {} (add a [[workspace.members]] entry)",
            missing.join(", ")
        ))
    }
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

/// Every member's version can be read. The release would otherwise treat a
/// member without one as 0.0.0.
fn check_member_manifests(ws: &WorkspaceConfig, root: &Path) -> CheckResult {
    let versions = workspace::member_versions(root).map_err(|e| e.to_string())?;
    let missing: Vec<&str> = ws
        .members
        .iter()
        .map(|m| m.name.as_str())
        .filter(|name| !versions.contains_key(*name))
        .collect();

    if missing.is_empty() {
        Ok(format!("versions read for {} members", ws.members.len()))
    } else {
        Err(format!("no version found for: {}", missing.join(", ")))
    }
}

fn check_version_tag(ctx: &ReleaseContext<'_>, config: &Config) -> CheckResult {
    match commits::latest_version_tag(ctx.backend, &config.release.tag_prefix) {
        Ok(Some(tag)) => Ok(format!("found {}", tag.name)),
        Ok(None) => Ok("none (first release mode)".to_owned()),
        Err(e) => Err(e.to_string()),
    }
}

/// Which members have a tag of their own; the rest are on their first release.
fn check_member_tags(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    ws: &WorkspaceConfig,
) -> CheckResult {
    let mut untagged = Vec::new();
    for member in &ws.members {
        let since =
            workspace::member_since(ctx.backend, member, &config.release.tag_prefix, "root()")
                .map_err(|e| e.to_string())?;
        if since == "root()" {
            untagged.push(member.name.as_str());
        }
    }

    let total = ws.members.len();
    let tagged = total - untagged.len();
    Ok(if untagged.is_empty() {
        format!("all {total} members tagged")
    } else {
        format!(
            "{tagged} of {total} members tagged; first release for: {}",
            untagged.join(", ")
        )
    })
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

/// Each member's manifest version against its latest crates.io release.
/// Like the single-crate check, a mismatch is reported, not failed.
fn check_members_on_crates_io(ws: &WorkspaceConfig, root: &Path) -> CheckResult {
    let versions = workspace::member_versions(root).map_err(|e| e.to_string())?;

    let mut out_of_sync = Vec::new();
    for member in &ws.members {
        let Some(version) = versions.get(&member.name) else {
            continue; // reported by the manifest check
        };
        let published = registry::latest_version_on_crates_io(&member.name)
            .map_err(|e| format!("{}: {e}", member.name))?;
        if let Some(published) = published.filter(|p| p != version) {
            out_of_sync.push(format!(
                "{} (v{published} published, manifest v{version})",
                member.name
            ));
        }
    }

    Ok(if out_of_sync.is_empty() {
        format!(
            "all {} members in sync with their manifests",
            ws.members.len()
        )
    } else {
        format!("out of sync: {}", out_of_sync.join(", "))
    })
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

fn check_bump(ctx: &ReleaseContext<'_>, config: &Config, since: &str) -> Result<String, String> {
    // If it's a first release, we bypass the bump check completely!
    if since == "root()" {
        return Ok("first release mode (version frozen)".to_owned());
    }

    let commits = ctx
        .backend
        .log_commits(&format!("{since}..@"))
        .map_err(|e| e.to_string())?;

    let bump =
        commits::resolve_bump(config.bump.force.as_deref(), &commits).map_err(|e| e.to_string())?;

    if bump == BumpKind::None {
        Err("no releasable commits (feat/fix/BREAKING) found since last tag".to_owned())
    } else {
        Ok("releasable commits found".to_owned())
    }
}

/// The members the release would publish, from the same plan the release uses.
fn check_member_bumps(ctx: &ReleaseContext<'_>, config: &Config, root: &Path) -> CheckResult {
    let prepared = prepare::plan_release(ctx.backend, ctx.manifest, config, root)
        .map_err(|e| e.to_string())?
        .ok_or("no member has releasable commits since its last tag")?;

    let mut released: Vec<&str> = prepared
        .members()
        .into_iter()
        .flatten()
        .filter(|(_, plan)| plan.is_released())
        .map(|(name, _)| name.as_str())
        .collect();
    released.sort_unstable();

    Ok(format!(
        "{} member(s) to release: {}",
        released.len(),
        released.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Versioning, WorkspaceConfig, WorkspaceMember};
    use std::fs;

    #[test]
    fn detects_member_missing_from_config() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\n",
        )
        .unwrap();
        for name in ["a", "b"] {
            fs::create_dir(dir.path().join(name)).unwrap();
            fs::write(
                dir.path().join(name).join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
            )
            .unwrap();
        }

        let config = Config {
            workspace: Some(WorkspaceConfig {
                enabled: true,
                versioning: Versioning::Unified,
                members: vec![WorkspaceMember {
                    name: "a".into(),
                    path: "a".into(),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let err = check_workspace_members(&config, dir.path()).unwrap_err();
        assert!(err.contains('b'));
    }
}
