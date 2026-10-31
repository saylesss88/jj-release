use std::{fs, path::Path};

use semver::Version;
use toml_edit::DocumentMut;

use crate::{
    commits,
    config::{Config, Versioning, WorkspaceConfig, WorkspaceMember},
    distro,
    errors::ReleaseError,
    errors::Result,
    manifest,
    pipeline::{self, PreparedRelease, ReleaseContext},
    workspace,
};

use jj_release_changelog::{self, ChangelogLayout, CommitInfo};

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

/// Prepends a new release section to the project's changelog.
///
/// If the configured changelog file does not exist, it will be created.
///
/// # Errors
///
/// Returns an error if the changelog file cannot be read from or written to disk.
pub fn update_changelog(
    ctx: &ReleaseContext<'_>,
    root: &Path,
    config: &Config,
    prepared: &pipeline::PreparedRelease,
) -> Result<()> {
    let single = || {
        write_section(
            root,
            config,
            &jj_release_changelog::render_changelog_section(
                &prepared.commits,
                Some(&prepared.next_version),
                &config.changelog,
                None,
            ),
        )
    };

    let Some(ws) = config.workspace.as_ref().filter(|w| w.enabled) else {
        return single();
    };

    match (config.changelog.layout, &ws.versioning) {
        // One shared version, one file: same as a single crate.
        (ChangelogLayout::Root, Versioning::Unified) => single(),

        // One file per member, each with only its own commits.
        (ChangelogLayout::PerMember, _) => {
            for (member, version, commits) in releasing_members(ctx, config, ws, prepared)? {
                let section = jj_release_changelog::render_changelog_section(
                    &commits,
                    Some(&version),
                    &config.changelog,
                    None,
                );
                write_section(&root.join(&member.path), config, &section)?;
            }
            Ok(())
        }

        // One root file with a named section per bumped member.
        (ChangelogLayout::Root, Versioning::Independent) => {
            let sections: Vec<String> = releasing_members(ctx, config, ws, prepared)?
                .into_iter()
                .map(|(member, version, commits)| {
                    let mut s = jj_release_changelog::render_heading(
                        Some(&member.name),
                        Some(&version),
                        None,
                    );
                    s.push_str(&jj_release_changelog::render_body(
                        &commits,
                        &config.changelog,
                    ));
                    s
                })
                .collect();
            if sections.is_empty() {
                return Ok(());
            }
            write_section(root, config, &sections.join("\n"))
        }
    }
}

/// Members being released in this run, with their next version and the
/// commits that touched their path. Members with no bump or no commits are
/// skipped.
fn releasing_members<'a>(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    ws: &'a WorkspaceConfig,
    prepared: &PreparedRelease,
) -> Result<Vec<(&'a WorkspaceMember, Version, Vec<CommitInfo>)>> {
    // Unified members share the root tag; look it up once.
    let root_since = commits::latest_version_tag(ctx.backend, &config.release.tag_prefix)?
        .map_or_else(|| "root()".to_owned(), |t| t.name);

    let mut out = Vec::new();
    for member in &ws.members {
        let Some(version) = member_next_version(ws, member, prepared) else {
            continue;
        };
        let since = match ws.versioning {
            Versioning::Unified => root_since.clone(),
            Versioning::Independent => {
                workspace::member_since(ctx.backend, member, &config.release.tag_prefix, "root()")?
            }
        };
        let commits = ctx
            .backend
            .log_commits_for_path(&format!("{since}..@"), &member.path)?;
        if !commits.is_empty() {
            out.push((member, version, commits));
        }
    }
    Ok(out)
}

/// The version `member` is being released as, or `None` if it isn't
/// being released (independent mode, no changes).
fn member_next_version(
    ws: &WorkspaceConfig,
    member: &WorkspaceMember,
    prepared: &PreparedRelease,
) -> Option<Version> {
    match ws.versioning {
        Versioning::Unified => Some(prepared.next_version.clone()),
        Versioning::Independent => {
            let (current, next) = prepared.member_bumps.as_ref()?.get(&member.name)?;
            (current != next).then(|| next.clone())
        }
    }
}

fn write_section(dir: &Path, config: &Config, section: &str) -> Result<()> {
    let path = dir.join(&config.changelog.file);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    fs::write(
        &path,
        jj_release_changelog::prepend_to_file(&existing, section),
    )?;
    Ok(())
}

/// Synchronizes cross-member dependency versions within a Cargo workspace.
///
/// Iterates through all workspace members and updates any local dependencies
/// on other workspace members to match the upcoming release version.
///
/// # Errors
///
/// Returns an error if a member's `Cargo.toml` cannot be read, contains
/// invalid TOML syntax, or cannot be written back to disk.
pub fn update_workspace_dependencies(
    root: &Path,
    config: &Config,
    next_version: &Version,
) -> Result<()> {
    let Some(ws) = &config.workspace else {
        return Ok(());
    };
    if !ws.enabled {
        return Ok(());
    }

    let next_str = next_version.to_string();

    for member in &ws.members {
        let cargo_toml = root.join(&member.path).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }

        let raw = fs::read_to_string(&cargo_toml)?;
        let mut doc: DocumentMut = raw
            .parse()
            .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", cargo_toml.display())))?;

        for other in &ws.members {
            manifest::bump_workspace_dependency(&mut doc, &other.name, &next_str);
        }

        fs::write(&cargo_toml, doc.to_string())?;
    }

    Ok(())
}

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
    if config.publish.cargo {
        let is_independent = config
            .workspace
            .as_ref()
            .is_some_and(|ws| matches!(ws.versioning, Versioning::Independent));

        if is_independent {
            publish_independent(ctx, config, prepared, root)?;
        } else {
            publish_unified(ctx, config, root)?;
        }
    }
    // Distro targets run after crates.io succeeds.
    #[cfg(any(feature = "publish-aur", feature = "publish-copr"))]
    if distro::any_configured(config) {
        let mut fatal = Vec::new();
        for (target, required, result) in distro::update_all(config, root, &prepared.next_version) {
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
    let ws = config.workspace.as_ref().ok_or_else(|| {
        ReleaseError::Message("workspace config required for independent publishing".into())
    })?;
    let bumps = prepared.member_bumps.as_ref().ok_or_else(|| {
        ReleaseError::Message("member bumps required for independent publishing".into())
    })?;

    for member in workspace::ordered_members(&ws.members)? {
        let Some((current, next)) = bumps.get(&member.name) else {
            continue;
        };
        // Only publish if the topological member actually received a bump
        if current == next {
            continue;
        }
        let tag = workspace::member_tag_name(member, next, &config.release.tag_prefix);
        let member_root = root.join(&member.path);

        // Bump version
        workspace::bump_member_version(root, &member.name, next)?;

        // Update cross-member dependencies
        let cargo_toml = member_root.join("Cargo.toml");
        if cargo_toml.exists() {
            let raw = fs::read_to_string(&cargo_toml)?;
            let mut doc: DocumentMut = raw.parse().map_err(|e| {
                ReleaseError::Message(format!("parsing {}: {e}", cargo_toml.display()))
            })?;
            for other in &ws.members {
                if let Some((_, other_next)) = bumps.get(&other.name) {
                    manifest::bump_workspace_dependency(
                        &mut doc,
                        &other.name,
                        &other_next.to_string(),
                    );
                }
            }
            fs::write(&cargo_toml, doc.to_string())?;
        }

        // Keep the lock file in step with the bumped manifests so it is part
        // of the release commit, not rewritten by `cargo publish` afterwards.
        ctx.manifest.sync_lockfile(root)?;

        // Commit, tag, export, push tag
        ctx.backend.new_commit(&format!("chore: release {tag}"))?;
        ctx.backend.create_tag(&tag, "@")?;
        ctx.backend.git_export()?;
        ctx.backend.git_push(None, Some(&tag))?;

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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, process::Command, rc::Rc};

    use super::*;
    use crate::{
        commits::BumpKind,
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
        fn list_tags(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
        fn check_identity(&self) -> Result<()> {
            Ok(())
        }
        fn log_commits_for_path(&self, _: &str, _: &str) -> Result<Vec<CommitInfo>> {
            Ok(vec![])
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

        let old = Version::new(0, 1, 0);
        let new = Version::new(0, 2, 0);
        let prepared = PreparedRelease {
            since: "v0.1.0".into(),
            current_version: old.clone(),
            baseline_version: old.clone(),
            next_version: new.clone(),
            tag_name: "v0.2.0".into(),
            commits: vec![],
            bump: BumpKind::Minor,
            // member_bumps: Some(std::iter::once(("a".to_owned(), (old, new))).collect()),
            member_bumps: Some(HashMap::from([("a".to_owned(), (old, new))])),
        };

        publish_independent(&ctx, &config, &prepared, root).unwrap();

        let calls = log.borrow().clone();
        assert_eq!(
            calls,
            [
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
