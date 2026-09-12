//! jj-release, semantic releases for Jujutsu repositories.

use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use jj_release::config::{self, Config};
use jj_release::forge::{ForgeBackend, ForgejoForge, GitHubForge, GitLabForge, NoForge};
use jj_release::jj::ShellBackend;
use jj_release::manifest::{CargoManifest, GoManifest, ManifestBackend, NpmManifest};
use jj_release::pipeline::{self, PreparedRelease, ReleaseContext};
use jj_release::publish::{CargoPublish, NoPublish, NpmPublish, PublishBackend};
use jj_release::workspace::WorkspaceManifest;
use jj_release::{changelog, jj, workspace};

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
    Changelog,
    /// Push a PR
    Pr,
    /// Auto-detect environment and generate a release.toml.
    Init,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Resolve repo root.
    let cwd = env::current_dir().context("getting current directory")?;
    let root = match &cli.repo {
        Some(p) => p.clone(),
        None => jj::find_repo_root(&cwd)?,
    };

    // Load config (falls back to defaults if release.toml absent).
    let config = config::load(&root)?;

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
        "github" => Box::new(GitHubForge),
        "gitlab" => Box::new(GitLabForge),
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
        Subcommand::Run => release_pipeline(&ctx, &config, &root, cli.dry_run, cli.quiet),
        Subcommand::NextVersion => pipeline::print_next_version(&ctx, &config, &root),
        Subcommand::Changelog => pipeline::print_changelog(&ctx, &config, &root),
        Subcommand::Pr => release_pr(&ctx, &config, &root, cli.quiet),
        Subcommand::Init => init(&root),
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
    if let Some(ws) = &config.workspace {
        if ws.enabled {
            let ordered = workspace::ordered_members(&ws.members)?;
            for member in ordered {
                info!("→ Publishing {}…", member.name);
                ctx.publisher
                    .publish(&root.join(&member.path), &config.publish.cargo_flags)?;
            }
        }
    } else {
        info!("→ Publishing…");
        ctx.publisher.publish(root, &config.publish.cargo_flags)?;
    }

    // 5. Forge release.
    if config.release.create_release {
        info!("→ Creating forge release {tag_name}…");
        ctx.forge.create_release(tag_name)?;
    }

    info!("✓ Released {tag_name}");
    Ok(())
}

fn release_pr(
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &std::path::Path,
    quiet: bool,
) -> Result<()> {
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

fn init(root: &std::path::Path) -> Result<()> {
    use jj_release::detect;

    let release_toml = root.join("release.toml");
    if release_toml.exists() {
        println!("release.toml already exists — delete it first to reinitialize.");
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
        let content = std::fs::read_to_string(&workspace_toml).unwrap_or_default();
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

    // Add workspace config if detected.
    if is_workspace {
        println!("→ Detected workspace");
        content.push_str(
            r#"
[workspace]
enabled = true
versioning = "unified"

# Add your workspace members below:
# [[workspace.members]]
# name = "mylib"
# path = "lib"
# publish = true
#
# [[workspace.members]]
# name = "mycli"
# path = "cli"
# publish = true
# depends_on = ["mylib"]
"#,
        );
    }

    std::fs::write(&release_toml, content)?;
    println!("✓ Written release.toml");

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
