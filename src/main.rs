//! jj-release, semantic releases for Jujutsu repositories.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::{borrow, env, fs};

use anyhow::{Context, Result};
use clap::Parser;

use jj_release::commits::BumpKind;
use jj_release::config::{Config, Versioning};
use jj_release::errors::CliError;
use jj_release::forge::{ForgeBackend, ForgejoForge, GitHubForge, GitLabForge, NoForge};
use jj_release::jj::ShellBackend;
use jj_release::manifest::{CargoManifest, GoManifest, ManifestBackend, NpmManifest};
use jj_release::pipeline::{PreparedRelease, ReleaseContext};
use jj_release::publish::{CargoPublish, NoPublish, NpmPublish, PublishBackend};
use jj_release::workspace::WorkspaceManifest;
use jj_release::{changelog, commits, config, detect, errors, jj, pipeline, workspace};

#[derive(Parser, Debug)]
#[command(
    name = "jj-release",
    version,
    about = "Semantic releases for Jujutsu repositories",
    long_about = None,
)]
struct Cli {
    /// Path to the repository root. Defaults to the nearest .jj/ ancestor.
    #[arg(long)]
    repo: Option<PathBuf>,

    /// Print what would happen without making any changes.
    #[arg(long, short = 'n')]
    dry_run: bool,

    /// Suppress non-essential output.
    #[arg(long, short)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Subcommand>,
}

#[derive(clap::Subcommand, Debug)]
enum Subcommand {
    /// Run the full release pipeline (default when no subcommand given).
    Run,
    /// Print the next version that would be released, then exit.
    NextVersion,
    /// Generate changelog for commits since last tag and print to stdout.
    #[command(name = "changelog")]
    Changelog {
        /// Write to a file instead of stdout. Creates or prepends to the file.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Push a PR
    Pr,
    /// Auto-detect environment and generate a release.toml.
    Init,
    /// Check that everything is ready for a release without making any changes.
    Validate,
}

fn main() {
    let code = errors::report(run());
    std::process::exit(code);
}

fn run() -> Result<(), errors::CliError> {
    let cli = Cli::parse();

    // Resolve repo root.
    let cwd = env::current_dir().context("getting current directory")?;
    let root = match &cli.repo {
        Some(p) => p.clone(),
        None => jj::find_repo_root(&cwd)?,
    };

    // Load config (falls back to defaults if release.toml absent).
    let config = config::load(&root)?;
    if !root.join("release.toml").exists() {
        // Using defaults, mention init for first-time users.
        eprintln!(
            "hint: no release.toml found, using defaults. Run `jj-release init` to customize"
        );
    }

    let publisher: Box<dyn PublishBackend> = match config.manifest_backend.as_str() {
        "npm" => Box::new(NpmPublish),
        _ if config.publish.cargo => Box::new(CargoPublish),
        _ => Box::new(NoPublish),
    };

    let manifest: Box<dyn ManifestBackend> = if config.workspace.as_ref().is_some_and(|w| w.enabled)
    {
        Box::new(WorkspaceManifest)
    } else {
        match config.manifest_backend.as_str() {
            "go" => Box::new(GoManifest),
            "npm" => Box::new(NpmManifest),
            _ => Box::new(CargoManifest),
        }
    };
    // Set up the backend.
    let backend = ShellBackend::new(&root)?;

    let forge: Box<dyn ForgeBackend> = match config.release.forge.as_str() {
        "github" => {
            if !detect::tool_available("gh") {
                return Err(CliError::missing_tool("gh"));
            }
            Box::new(GitHubForge)
        }
        "gitlab" => {
            if !detect::tool_available("glab") {
                return Err(CliError::missing_tool("glab"));
            }

            Box::new(GitLabForge)
        }
        "forgejo" => Box::new(ForgejoForge {
            host: config.release.forge_url.clone(),
            token: std::env::var("FORGEJO_TOKEN").unwrap_or_default(),
            owner: String::new(), // TODO: parse from git remote
            repo: String::new(),  // TODO: parse from git remote
        }),
        _ => Box::new(NoForge),
    };

    let ctx = ReleaseContext {
        backend: &backend,
        manifest: manifest.as_ref(),
        forge: forge.as_ref(),
        publisher: publisher.as_ref(),
    };

    match cli.command.unwrap_or(Subcommand::Run) {
        Subcommand::Run => Ok(release_pipeline(
            &ctx,
            &config,
            &root,
            cli.dry_run,
            cli.quiet,
        )?),
        Subcommand::NextVersion => Ok(pipeline::print_next_version(&ctx, &config, &root)?),
        Subcommand::Changelog { output } => Ok(pipeline::print_changelog(
            &ctx,
            &config,
            &root,
            output.as_deref(),
        )?),
        Subcommand::Pr => Ok(release_pr(&ctx, &config, &root, cli.quiet)?),
        Subcommand::Init => Ok(init(&root)?),
        Subcommand::Validate => Ok(pipeline::validate(&ctx, &config, &root)?),
    }
}

// -- Pipeline --

fn release_pipeline(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &std::path::Path,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let Some(prepared) = pipeline::prepare_release(ctx.backend, ctx.manifest, config, root)? else {
        info!("No trigger commit found or no releasable commits. Nothing to release.");
        return Ok(());
    };

    let PreparedRelease {
        next_version,
        tag_name,
        commits,
        ..
    } = &prepared;

    info!("  Current version: {}", prepared.current_version);
    info!(
        "  Bump: {:?} → {next_version}  (tag: {tag_name})",
        prepared.bump
    );

    if dry_run {
        println!("[dry-run] Would release {next_version} as {tag_name}");
        if let Some(ws) = &config.workspace {
            if ws.enabled {
                let ordered = workspace::ordered_members(&ws.members)?;
                println!("[dry-run] Would publish in order:");
                for member in ordered {
                    println!("  - {} ({})", member.name, member.path);
                }
            }
        } else {
            println!("[dry-run] Would publish from root");
        }
        return Ok(());
    }

    // 1. Write changelog.
    if config.changelog.enabled {
        info!("→ Writing changelog…");
        let changelog_path = root.join(&config.changelog.file);
        let existing = if changelog_path.exists() {
            std::fs::read_to_string(&changelog_path)?
        } else {
            String::new()
        };
        let section = changelog::render_changelog_section(commits, next_version);
        let updated = changelog::prepend_to_file(&existing, &section);
        std::fs::write(&changelog_path, updated)?;
    }

    // 2. Bump version and create release commit.
    info!("→ Bumping version to {next_version}…");
    ctx.manifest.write_version(root, next_version)?;
    let release_message = format!("chore: release {tag_name}");
    info!("→ Creating commit {:?}…", release_message);
    ctx.backend.new_commit(&release_message)?;

    // 3. Tag, bookmark, push.
    info!("→ Creating tag {tag_name}…");
    ctx.backend.create_tag(tag_name, "@")?;
    info!("→ Moving bookmark {:?} to @…", config.release.bookmark);
    ctx.backend.set_bookmark(&config.release.bookmark, "@")?;
    info!("→ Exporting to git…");
    ctx.backend.git_export()?;
    info!("→ Pushing bookmark and tags…");
    ctx.backend
        .git_push(&config.release.bookmark, Some(tag_name))?;

    // 4. Publish.
    run_publish(ctx, config, root, &prepared, quiet)?;

    // 5. Forge release.
    if config.release.create_release {
        info!("→ Creating forge release {tag_name}…");
        ctx.forge.create_release(tag_name)?;
    }

    info!("✓ Released {tag_name}");
    Ok(())
}

fn release_pr(ctx: &ReleaseContext<'_>, config: &Config, root: &Path, quiet: bool) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let Some(prepared) = pipeline::prepare_release(ctx.backend, ctx.manifest, config, root)? else {
        info!("No trigger commit found or no releasable commits. Nothing to release.");
        return Ok(());
    };

    info!("  Found trigger commit.");
    info!("  Current version: {}", prepared.current_version);
    info!(
        "  Bump: {:?} → {}  (tag: {})",
        prepared.bump, prepared.next_version, prepared.tag_name
    );

    // Generate changelog preview for PR body — no file write.
    let body = changelog::render_changelog_section(&prepared.commits, &prepared.next_version);

    // Push to a release bookmark so PR has something to merge into main.
    let pr_bookmark = format!("release/{}", prepared.tag_name);
    info!("→ Creating bookmark {pr_bookmark}…");
    ctx.backend.set_bookmark(&pr_bookmark, "@")?;
    info!("→ Exporting to git…");
    ctx.backend.git_export()?;
    info!("→ Pushing {pr_bookmark}…");
    ctx.backend.git_push(&pr_bookmark, None)?;
    info!("→ Opening PR…");
    ctx.forge.create_pr(
        &prepared.tag_name,
        &pr_bookmark,
        &config.release.bookmark,
        &body,
    )?;

    info!("✓ PR opened for {}", prepared.tag_name);
    Ok(())
}

fn init(root: &Path) -> Result<()> {
    use jj_release::detect;

    let release_toml = root.join("release.toml");
    if release_toml.exists() {
        println!("release.toml already exists. Delete it first to reinitialize.");
        return Ok(());
    }

    // Detect forge.
    let forge = detect::detect_forge(root).unwrap_or("github");
    println!("→ Detected forge: {forge}");

    // Check CLI tool availability.
    if forge == "github" && !detect::tool_available("gh") {
        eprintln!("warning: forge is github but gh CLI not found on PATH");
        eprintln!("  install: https://cli.github.com");
    }
    if forge == "gitlab" && !detect::tool_available("glab") {
        eprintln!("warning: forge is gitlab but glab CLI not found on PATH");
        eprintln!("  install: https://gitlab.com/gitlab-org/cli");
    }

    // Detect language.
    let language = detect::detect_language(root).unwrap_or("cargo");
    println!("→ Detected language: {language}");

    // Detect workspace.
    let workspace_toml = root.join("Cargo.toml");
    let is_workspace = workspace_toml.exists() && {
        let content = fs::read_to_string(&workspace_toml).unwrap_or_default();
        content.contains("[workspace]")
    };

    // Build the config.
    let mut content = format!(
        r#"# Generated by jj-release init

[release]
forge = "{forge}"
create_release = false

[publish]
cargo = {}

[changelog]
enabled = true

manifest_backend = "{language}"
"#,
        language == "cargo"
    );

    // Detect workspace members from Cargo.toml.
    if is_workspace {
        println!("→ Detected workspace");

        // Try to parse members from [workspace] table.
        let members = parse_workspace_members(&workspace_toml);

        let member_config = if members.is_empty() {
            r#"
# Add your workspace members below:
# [[workspace.members]]
# name = "mylib"
# path = "lib"
# publish = true
"#
            .to_owned()
        } else {
            let mut s = String::new();
            for (name, path) in &members {
                println!("  → Found member: {name} ({path})");
                let _ = write!(
    s,
                    "\n[[workspace.members]]\nname = \"{name}\"\npath = \"{path}\"\npublish = true\n# depends_on = [] # add names of members this depends on\n"

);
            }
            s
        };

        let _ = write!(
            content,
            "\n[workspace]\nenabled = true\nversioning = \"unified\"\n{member_config}"
        );
    }

    fs::write(&release_toml, content)?;
    println!("✓ Written release.toml");

    Ok(())
}

fn parse_workspace_members(cargo_toml: &Path) -> Vec<(String, String)> {
    let Ok(raw) = fs::read_to_string(cargo_toml) else {
        return vec![];
    };
    let Ok(doc) = raw.parse::<toml_edit::DocumentMut>() else {
        return vec![];
    };
    let Some(members) = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return vec![];
    };

    members
        .iter()
        .filter_map(|m| m.as_str())
        .map(|path| {
            let member_toml = cargo_toml.parent().unwrap().join(path).join("Cargo.toml");
            let raw = fs::read_to_string(&member_toml).ok();
            let doc = raw
                .as_deref()
                .and_then(|r| r.parse::<toml_edit::DocumentMut>().ok());

            let name = doc
                .as_ref()
                .and_then(|d: &toml_edit::DocumentMut| {
                    d.get("package")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .map(borrow::ToOwned::to_owned)
                })
                .unwrap_or_else(|| path.split('/').next_back().unwrap_or(path).to_owned());

            (name, path.to_owned())
        })
        .collect::<Vec<_>>()
}

fn run_publish(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &std::path::Path,
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
                    .unwrap_or_else(|| semver::Version::new(0, 0, 0));
                let next = commits::apply_bump(&current, bump);
                info!("→ Bumping {} to {next}…", member.name);
                workspace::bump_member_version(root, &member.name, &next)?;
                info!("→ Publishing {}…", member.name);
                ctx.publisher
                    .publish(&root.join(&member.path), &config.publish.cargo_flags)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    #[test]
    fn pr_bookmark_name() {
        let config = Config::default();
        let version = Version::parse("0.2.0").unwrap();
        let tag = config.tag_name(&version);
        let bookmark = format!("release/{tag}");
        assert_eq!(bookmark, "release/v0.2.0");
    }
}
