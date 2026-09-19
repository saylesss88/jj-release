use std::{fs, path::Path};

use anyhow::Result;
use semver::Version;

use jj_release::{
    changelog,
    commits::{self, BumpKind},
    config::{Config, Versioning},
    manifest,
    pipeline::{self, PreparedRelease, ReleaseContext},
    registry, workspace,
};

pub fn release_pipeline(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let Some(prepared) = pipeline::prepare_release(ctx.backend, ctx.manifest, config, root)? else {
        info!("No trigger commit found or no releasable commits. Nothing to release.");
        info!("");
        info!("hint: To trigger a release, create a commit with the exact message:");
        info!("      jj new -m {:?}", config.release.trigger);
        info!("      (Run `jj-release --help` for full usage details)");
        return Ok(());
    };

    let PreparedRelease {
        next_version,
        tag_name,
        commits,
        ..
    } = &prepared;
    let is_independent = config
        .workspace
        .as_ref()
        .is_some_and(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent));

    info!("  Current version: {}", prepared.current_version);

    if !is_independent && prepared.baseline_version != prepared.current_version {
        info!("  crates.io version: {}", prepared.baseline_version);
        info!("  (using crates.io version as bump baseline)");
    }

    if dry_run {
        return print_dry_run(&prepared, config);
    }

    // Run pre-flight checks on all targets before touching anything
    info!(" → Running pre-flight checks (dry-run)...");
    if is_independent {
        if let Some(ws) = &config.workspace {
            for member in workspace::ordered_members(&ws.members)? {
                ctx.publisher.check(&root.join(&member.path))?;
            }
        }
    } else {
        ctx.publisher.check(root)?;
    }

    if !is_independent {
        // Write changelog.
        if config.changelog.enabled {
            info!("→ Writing changelog…");
            let changelog_path = root.join(&config.changelog.file);
            let existing = if changelog_path.exists() {
                fs::read_to_string(&changelog_path)?
            } else {
                String::new()
            };
            let section = changelog::render_changelog_section(commits, next_version);
            let updated = changelog::prepend_to_file(&existing, &section);
            fs::write(&changelog_path, updated)?;
        }

        // Bump version and create release commit.
        info!("→ Bumping version to {next_version}…");
        ctx.manifest.write_version(root, next_version)?;
        let release_message = format!("chore: release {tag_name}");
        info!("→ Creating commit {:?}…", release_message);
        ctx.backend.new_commit(&release_message)?;

        // Tag and bookmark locally
        info!("→ Creating tag {tag_name}…");
        ctx.backend.create_tag(tag_name, "@")?;
        info!("→ Moving bookmark {:?} to @…", config.release.bookmark);
        ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
        info!("→ Exporting to git…");
        ctx.backend.git_export()?;
    }

    // Publish to registry
    run_publish(ctx, config, root, &prepared, quiet)?;

    // Push to remote
    if is_independent {
        info!("→ Moving bookmark {:?} to @…", config.release.bookmark);
        ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
        info!("→ Exporting to git…");
        ctx.backend.git_export()?;
        info!("→ Pushing bookmark…");
        ctx.backend.git_push(Some(&config.release.bookmark), None)?;
    } else {
        info!("→ Pushing bookmark and tags...");
        ctx.backend
            .git_push(Some(&config.release.bookmark), Some(tag_name))?;
    }

    // Forge release.
    if !is_independent && config.release.create_release {
        info!("→ Creating forge release {tag_name}…");
        ctx.forge.create_release(tag_name)?;
    }

    info!("✓ Released {tag_name}");
    Ok(())
}

fn run_publish(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
    prepared: &PreparedRelease,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let Some(ws) = &config.workspace else {
        info!("→ Publishing…");
        return ctx.publisher.publish(root, &config.publish.cargo_flags);
    };

    if !ws.enabled {
        info!("→ Publishing…");
        return ctx.publisher.publish(root, &config.publish.cargo_flags);
    }

    let ordered = workspace::ordered_members(&ws.members)?;

    match ws.versioning {
        Versioning::Unified => {
            for member in &ordered {
                info!("→ Publishing {}…", member.name);
                ctx.publisher
                    .publish(&root.join(&member.path), &config.publish.cargo_flags)?;
            }
        }
        Versioning::Independent => {
            let since = prepared.since.clone();
            let bumps = workspace::member_bumps(
                ctx.backend,
                &ws.members,
                &since,
                config.bump.force.as_ref(),
                &config.release.tag_prefix,
            )?;
            let versions = workspace::member_versions(root)?;
            for member in &ordered {
                let bump = bumps.get(&member.name).copied().unwrap_or(BumpKind::None);
                if bump == BumpKind::None {
                    info!("→ Skipping {} (no releasable commits)…", member.name);
                    continue;
                }
                let current = versions
                    .get(&member.name)
                    .cloned()
                    .unwrap_or_else(|| Version::new(0, 0, 0));
                let next = commits::apply_bump(&current, bump);
                // Check this member's version before publishing.
                let member_cargo_toml = root.join(&member.path).join("Cargo.toml");
                if let Ok(name) = manifest::read_name(&member_cargo_toml)
                    && registry::version_exists_on_crates_io(&name, &next)?
                {
                    info!("→ Skipping {} : v{next} already published", member.name);
                    continue;
                }
                let tag = workspace::member_tag_name(member, &next, &config.release.tag_prefix);
                info!("→ Bumping {} to {next}…", member.name);
                workspace::bump_member_version(root, &member.name, &next)?;
                info!("→ Creating commit for {}…", member.name);
                ctx.backend.new_commit(&format!("chore: release {tag}"))?;
                info!("→ Creating tag {tag}...");
                ctx.backend.create_tag(&tag, "@")?;
                info!("→ Publishing {}…", member.name);
                ctx.publisher
                    .publish(&root.join(&member.path), &config.publish.cargo_flags)?;
                info!("→ Pushing tag {tag}...");
                ctx.backend.git_push(None, Some(&tag))?;
            }
        }
    }
    Ok(())
}

fn print_dry_run(prepared: &PreparedRelease, config: &Config) -> Result<()> {
    let is_independent = config
        .workspace
        .as_ref()
        .is_some_and(|ws| ws.enabled && matches!(ws.versioning, Versioning::Independent));

    if is_independent {
        println!("[dry-run] Independent workspace release:");
    } else {
        println!(
            "[dry-run] Would release {} as {}",
            prepared.next_version, prepared.tag_name
        );
    }

    if let Some(ws) = &config.workspace
        && ws.enabled
    {
        let ordered = workspace::ordered_members(&ws.members)?;
        println!("[dry-run] Would publish in order:");
        match ws.versioning {
            Versioning::Independent => {
                for member in ordered {
                    if let Some(ref mb) = prepared.member_bumps
                        && let Some((current, next)) = mb.get(&member.name)
                    {
                        if current == next {
                            println!(
                                "  - {} ({}) {} (no changes, skipping)",
                                member.name, member.path, current
                            );
                        } else {
                            let tag = workspace::member_tag_name(
                                member,
                                next,
                                &config.release.tag_prefix,
                            );
                            println!(
                                "  - {} ({}) {} → {} (tag: {})",
                                member.name, member.path, current, next, tag
                            );
                        }
                        continue;
                    }
                    println!("  - {} ({})", member.name, member.path);
                }
            }
            Versioning::Unified => {
                for member in ordered {
                    println!(
                        "  - {} ({}) → {}",
                        member.name, member.path, prepared.next_version
                    );
                }
            }
        }
        return Ok(());
    }

    println!("[dry-run] Would publish from root");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use std::cell::RefCell;

    use jj_release::{
        commits::CommitInfo, forge::NoForge, jj::JjBackend, manifest::ManifestBackend,
        publish::PublishBackend,
    };

    struct DummyManifest;

    impl ManifestBackend for DummyManifest {
        fn read_version(&self, _root: &Path) -> Result<Version> {
            Ok(Version::parse("0.1.0").unwrap())
        }
        fn write_version(&self, _root: &Path, _version: &Version) -> Result<()> {
            Ok(())
        }
    }

    struct FailPublish;

    impl PublishBackend for FailPublish {
        fn check(&self, _root: &Path) -> Result<()> {
            Ok(()) // Pre-flight succeeds
        }
        fn publish(&self, _root: &Path, _flags: &[String]) -> Result<()> {
            bail!("simulated crates.io outage") // Publish fails
        }
    }

    #[derive(Default)]
    struct TrackBackend {
        calls: RefCell<Vec<String>>,
    }

    impl JjBackend for TrackBackend {
        fn check_identity(&self) -> Result<()> {
            Ok(())
        }
        fn list_tags(&self) -> Result<Vec<String>> {
            Ok(vec!["v0.1.0".into()])
        }

        // Feed the pipeline a trigger and a feature commit so it attempts a release
        fn log_commits(&self, _revset: &str) -> Result<Vec<CommitInfo>> {
            Ok(vec![
                CommitInfo {
                    change_id: "1".into(),
                    description: "Release: please".into(),
                },
                CommitInfo {
                    change_id: "2".into(),
                    description: "feat: new stuff".into(),
                },
            ])
        }
        fn log_commits_for_path(&self, _r: &str, _p: &str) -> Result<Vec<CommitInfo>> {
            Ok(vec![])
        }
        fn new_commit(&self, _message: &str) -> Result<String> {
            Ok("abc".into())
        }

        // Track local tag creation
        fn create_tag(&self, _tag: &str, _revision: &str) -> Result<()> {
            self.calls.borrow_mut().push("tag".into());
            Ok(())
        }
        fn set_bookmark(&self, _name: &str, _revision: &str) -> Result<()> {
            Ok(())
        }

        // Track remote push
        fn git_push(&self, _bookmark: Option<&str>, _tag: Option<&str>) -> Result<()> {
            self.calls.borrow_mut().push("push".into());
            Ok(())
        }
        fn git_export(&self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn pipeline_aborts_before_push_if_publish_fails() {
        let backend = TrackBackend::default();
        let manifest = DummyManifest;
        let forge = NoForge;
        let publisher = FailPublish;

        let ctx = ReleaseContext::new(&backend, &manifest, &forge, &publisher);

        let mut config = Config::default();
        config.changelog.enabled = false; // Disable FS I/O for the test

        // Run the pipeline
        let result = release_pipeline(&ctx, &config, Path::new("/tmp"), false, true);

        // Assert pipeline failed
        assert!(result.is_err(), "Pipeline should fail when publish fails");

        // Assert local state mutated but remote state didn't
        let calls = backend.calls.borrow();
        assert!(calls.contains(&"tag".into()), "Local tag should be created");
        assert!(
            !calls.contains(&"push".into()),
            "Remote push MUST NOT be called"
        );
    }
}
