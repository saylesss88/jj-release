use std::{fmt::Write, fs, path::Path};

use toml_edit::DocumentMut;

use crate::{
    ReleasePlan, UnifiedPlan,
    config::{Config, WorkspaceMember},
    errors::ReleaseError,
    errors::Result,
    manifest,
    pipeline::{PreparedRelease, ReleaseContext},
    workspace,
};

/// Publishes the release to the configured package registry.
///
/// Routes the publish operation to either the unified or independent
/// workspace strategy based on the repository configuration. No-ops if
/// publishing is disabled.
///
/// # Errors
///
/// Returns an error if the underlying publish command (e.g., `cargo publish`)
/// fails, or if an independent workspace contains circular dependencies.
pub fn run_publish(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    prepared: &PreparedRelease,
) -> Result<()> {
    if !config.publish.cargo {
        return Ok(());
    }

    match &prepared.plan {
        ReleasePlan::Independent(_) => publish_independent(ctx, config, prepared, root),
        ReleasePlan::Unified(_) => publish_unified(ctx, config, root),
    }?;

    Ok(())
}

/// Update AUR and COPR packages for a released version. Runs after the
/// crates.io publish and git push, since the packages download the
/// published crate.
///
/// # Errors
///
/// Returns an error if a target with `fail_on_error = true` fails.
#[allow(unused_variables)] // unused when no distro features are enabled
pub fn run_distro(config: &Config, root: &Path, prepared: &PreparedRelease) -> Result<()> {
    #[cfg(any(feature = "publish-aur", feature = "publish-copr"))]
    {
        use crate::distro;
        if !distro::any_configured(config) {
            return Ok(());
        }
        let Some(plan) = prepared.unified() else {
            eprintln!(
                "warning: AUR/COPR publishing isn't supported for independent workspaces yet; skipping"
            );
            return Ok(());
        };

        let mut fatal = Vec::new();
        for (target, required, result) in distro::update_all(config, root, &plan.next_version) {
            match result {
                Ok(()) => eprintln!("✓ {target} updated"),
                Err(e) if required => {
                    eprintln!("✗ {target} failed: {e}");
                    fatal.push(target);
                }
                Err(e) => eprintln!("⚠ {target} failed (not required): {e}"),
            }
        }
        if !fatal.is_empty() {
            return Err(ReleaseError::Message(format!(
                "required distro target(s) failed (crates.io was already published): {}",
                fatal.join(", ")
            )));
        }
    }
    Ok(())
}

pub(super) fn publish_independent(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    prepared: &PreparedRelease,
    root: &Path,
) -> Result<()> {
    commit_and_tag_members(ctx, config, prepared, root)?;

    if config.publish.cargo {
        publish_members(ctx, config, prepared, root)?;
    }

    Ok(())
}

fn commit_and_tag_members(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    prepared: &PreparedRelease,
    root: &Path,
) -> Result<()> {
    let ws = config.workspace.as_ref().ok_or_else(|| {
        ReleaseError::Message("workspace config required for independent publishing".into())
    })?;
    let bumps = prepared.members().ok_or_else(|| {
        ReleaseError::Message("member bumps required for independent publishing".into())
    })?;

    let members = workspace::ordered_members(&ws.members)?;

    // PRE-FLIGHT CHECK: Ensure no tags we plan to create already exist.
    // This loops through all crates first, failing fast before touching any files.
    for member in &members {
        if let Some(plan) = bumps.get(&member.name)
            && plan.is_released()
        {
            let tag = workspace::member_tag_name(member, &plan.next, &config.release.tag_prefix);

            if ctx.backend.tag_exists(&tag)? {
                return Err(ReleaseError::Message(format!(
                    "Refusing to proceed: Tag '{tag}' already exists for member '{}'.",
                    member.name
                )));
            }
        }
    }

    // Phase 1: Bump versions, commit, tag, and push to VCS.
    for member in members {
        let Some(plan) = bumps.get(&member.name) else {
            continue;
        };
        if !plan.is_released() {
            continue;
        }

        let next = &plan.next;
        let tag = workspace::member_tag_name(member, next, &config.release.tag_prefix);
        let member_root = root.join(&member.path);

        workspace::bump_member_version(root, &member.name, next)?;

        let cargo_toml = member_root.join("Cargo.toml");
        if cargo_toml.exists() {
            let raw = fs::read_to_string(&cargo_toml)?;
            let mut doc: DocumentMut = raw.parse().map_err(|e| {
                ReleaseError::Message(format!("parsing {}: {e}", cargo_toml.display()))
            })?;
            for other in &ws.members {
                if let Some(other_plan) = bumps.get(&other.name) {
                    manifest::bump_workspace_dependency(
                        &mut doc,
                        &other.name,
                        &other_plan.next.to_string(),
                    );
                }
            }
            fs::write(&cargo_toml, doc.to_string())?;
        }

        ctx.manifest.sync_lockfile(root)?;

        ctx.backend.new_commit(&format!("chore: release {tag}"))?;
        ctx.backend.create_tag(&tag, "@")?;
        ctx.backend.git_export()?;
        ctx.backend.git_push(None, Some(&tag))?;
    }

    Ok(())
}

fn publish_members(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    prepared: &PreparedRelease,
    root: &Path,
) -> Result<()> {
    let ws = config.workspace.as_ref().ok_or_else(|| {
        ReleaseError::Message("workspace config required for independent publishing".into())
    })?;
    let bumps = prepared.members().ok_or_else(|| {
        ReleaseError::Message("member bumps required for independent publishing".into())
    })?;

    for member in workspace::ordered_members(&ws.members)? {
        let Some(plan) = bumps.get(&member.name) else {
            continue;
        };
        if !plan.is_released() {
            continue;
        }

        let member_root = root.join(&member.path);
        ctx.publisher
            .publish(&member_root, &config.publish.cargo_flags)?;
    }

    Ok(())
}

pub(super) fn publish_unified(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
) -> Result<()> {
    if let Some(ws) = config
        .workspace
        .as_ref()
        .filter(|w| w.enabled && !w.members.is_empty())
    {
        for member in workspace::ordered_members(&ws.members)?
            .into_iter()
            .filter(|m| m.publish)
        {
            ctx.publisher
                .publish(&root.join(&member.path), &config.publish.cargo_flags)?;
        }
        return Ok(());
    }

    // For single crates or unified workspaces, we just publish from the project root
    ctx.publisher.publish(root, &config.publish.cargo_flags)
}

pub(super) fn unified_plan(prepared: &PreparedRelease) -> Result<&UnifiedPlan> {
    prepared.unified().ok_or_else(|| {
        ReleaseError::Message("internal error: expected a unified release plan".into())
    })
}

pub(super) fn dependency_notes(member: &WorkspaceMember, prepared: &PreparedRelease) -> String {
    let Some(members) = prepared.members() else {
        return String::new();
    };
    let Some(plan) = members.get(&member.name) else {
        return String::new();
    };
    if plan.updated_deps.is_empty() {
        return String::new();
    }

    let mut out = String::from("\n### Dependencies\n\n");
    for dep in &plan.updated_deps {
        if let Some(dep_plan) = members.get(dep) {
            let _ = writeln!(out, "- Updated `{dep}` to {}", dep_plan.next);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, process::Command, rc::Rc};

    use semver::Version;

    use super::*;
    use crate::{
        MemberPlan, Versioning,
        config::{WorkspaceConfig, WorkspaceMember},
        forge::NoForge,
        jj::JjBackend,
        manifest::{ManifestBackend, WorkspaceManifest},
        publish::NoPublish,
    };

    use jj_release_changelog::CommitInfo;

    type Log = Rc<RefCell<Vec<String>>>;

    /// Real workspace manifest that records when the lock file is synced.
    struct RecordingManifest(Log);

    impl ManifestBackend for RecordingManifest {
        fn read_version(&self, root: &Path) -> Result<Version> {
            WorkspaceManifest.read_version(root)
        }
        fn write_version(&self, root: &Path, version: &Version) -> Result<()> {
            WorkspaceManifest.write_version(root, version)
        }
        fn sync_lockfile(&self, root: &Path) -> Result<()> {
            self.0.borrow_mut().push("sync_lockfile".into());
            WorkspaceManifest.sync_lockfile(root)
        }
    }

    /// Backend that records calls, and checks the lock file is already
    /// consistent at the moment the release commit is created.
    struct RecordingBackend {
        log: Log,
        root: std::path::PathBuf,
    }

    impl JjBackend for RecordingBackend {
        fn log_commits(&self, _: &str) -> Result<Vec<CommitInfo>> {
            Ok(vec![])
        }
        fn new_commit(&self, message: &str) -> Result<String> {
            let in_sync = Command::new("cargo")
                .args(["metadata", "--locked", "--offline", "--format-version", "1"])
                .current_dir(&self.root)
                .output()
                .unwrap()
                .status
                .success();
            assert!(
                in_sync,
                "Cargo.lock was stale when the release commit was made"
            );
            self.log.borrow_mut().push(format!("new_commit {message}"));
            Ok(String::new())
        }
        fn create_tag(&self, tag: &str, _: &str) -> Result<()> {
            self.log.borrow_mut().push(format!("create_tag {tag}"));
            Ok(())
        }
        fn set_bookmark(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn git_push(&self, _: Option<&str>, tag: Option<&str>) -> Result<()> {
            self.log
                .borrow_mut()
                .push(format!("git_push {}", tag.unwrap_or("-")));
            Ok(())
        }
        fn git_export(&self) -> Result<()> {
            Ok(())
        }
        fn list_tags(&self, _revset: &str) -> Result<Vec<String>> {
            Ok(vec![])
        }
        fn check_identity(&self) -> Result<()> {
            Ok(())
        }
        fn log_commits_for_path(&self, _: &str, _: &str) -> Result<Vec<CommitInfo>> {
            Ok(vec![])
        }
        fn commit_date(&self, _rev: &str) -> Result<String> {
            Ok("2026-01-01".to_owned())
        }
        fn tag_exists(&self, tag: &str) -> Result<bool> {
            // Record that the pre-flight check happened
            self.log.borrow_mut().push(format!("tag_exists {tag}"));

            // Return false so the release process continues.
            // (If it returned true, commit_and_tag_members would abort!)
            Ok(false)
        }
    }

    #[test]
    fn independent_release_syncs_lockfile_before_the_release_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("a/src")).unwrap();
        fs::write(
            root.join("a/Cargo.toml"),
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(root.join("a/src/lib.rs"), "").unwrap();
        assert!(
            Command::new("cargo")
                .args(["generate-lockfile", "--offline"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );

        let log: Log = Rc::default();
        let backend = RecordingBackend {
            log: Rc::clone(&log),
            root: root.to_path_buf(),
        };
        let manifest = RecordingManifest(Rc::clone(&log));
        let ctx = ReleaseContext::new(&backend, &manifest, &NoForge, &NoPublish);

        let config = Config {
            workspace: Some(WorkspaceConfig {
                enabled: true,
                versioning: Versioning::Independent,
                members: vec![WorkspaceMember {
                    name: "a".into(),
                    path: "a".into(),
                    ..WorkspaceMember::default()
                }],
            }),
            ..Config::default()
        };

        let prepared = PreparedRelease {
            since: "v0.1.0".into(),
            commits: vec![],
            plan: ReleasePlan::Independent(HashMap::from([(
                "a".to_owned(),
                MemberPlan {
                    current: Version::new(0, 1, 0),
                    next: Version::new(0, 2, 0),
                    first_release: false,
                    propagated: false,
                    updated_deps: Vec::new(),
                },
            )])),
        };

        publish_independent(&ctx, &config, &prepared, root).unwrap();

        let calls = log.borrow().clone();
        assert_eq!(
            calls,
            [
                "tag_exists v0.2.0",
                "sync_lockfile",
                "new_commit chore: release v0.2.0",
                "create_tag v0.2.0",
                "git_push v0.2.0",
            ]
        );
        let lock = fs::read_to_string(root.join("Cargo.lock")).unwrap();
        assert!(lock.contains("name = \"a\"\nversion = \"0.2.0\""), "{lock}");
    }
}
