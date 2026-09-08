//! jj-release, semantic releases for Jujutsu repositories.

mod changelog;
mod commits;
mod config;
mod jj;
mod manifest;

use std::env;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Parser;

use commits::{apply_bump, compute_bump, find_trigger, latest_version_tag, BumpKind};
use config::Config;
use jj::{find_repo_root, JjBackend, ShellBackend};
use manifest::{read_version, write_version};

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
        None => find_repo_root(&cwd)?,
    };

    // Load config (falls back to defaults if release.toml absent).
    let config = config::load(&root)?;

    // Set up the backend.
    let backend = ShellBackend::new(&root)?;

    match cli.command.unwrap_or(Subcommand::Run) {
        Subcommand::Run => release_pipeline(&backend, &config, &root, cli.dry_run, cli.quiet),
        Subcommand::NextVersion => print_next_version(&backend, &config, &root),
        Subcommand::Changelog => print_changelog(&backend, &config, &root),
    }
}

fn print_changelog(backend: &dyn JjBackend, config: &Config, root: &std::path::Path) -> Result<()> {
    let cargo_toml = root.join("Cargo.toml");
    let current = read_version(&cargo_toml)?;
    let since = match latest_version_tag(backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };
    let commits = backend.log_commits(&format!("{since}..@"))?;
    let next = apply_bump(&current, compute_bump(&commits));
    let section = changelog::render_changelog_section(&commits, &next);
    print!("{section}");
    Ok(())
}
// -- Pipeline --

fn release_pipeline(
    backend: &dyn JjBackend,
    config: &Config,
    root: &std::path::Path,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let since = match latest_version_tag(backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };

    // 1. Detect trigger commit.
    info!(
        "→ Scanning for trigger commit {:?}…",
        config.release.trigger
    );
    let Some(_trigger_id) = find_trigger(backend, &config.release.trigger, &since)? else {
        info!("No trigger commit found. Nothing to release.");
        return Ok(());
    };
    info!("  Found trigger commit.");

    // 2. Read current version.
    let cargo_toml = root.join("Cargo.toml");
    let current_version =
        read_version(&cargo_toml).context("reading current version from Cargo.toml")?;
    info!("  Current version: {current_version}");

    // 3. Fetch commits and compute bump — reuse commits for changelog.
    let commits = backend.log_commits(&format!("{since}..@"))?;
    let bump = if let Some(force) = &config.bump.force {
        match force.as_str() {
            "major" => BumpKind::Major,
            "minor" => BumpKind::Minor,
            "patch" => BumpKind::Patch,
            other => bail!("unknown bump.force value {other:?} — must be major/minor/patch"),
        }
    } else {
        compute_bump(&commits)
    };

    if bump == BumpKind::None {
        info!("No releasable commits found since last tag. Nothing to release.");
        return Ok(());
    }

    let next_version = apply_bump(&current_version, bump);
    let tag_name = config.tag_name(&next_version);
    info!("  Bump: {bump:?} → {next_version}  (tag: {tag_name})");

    if dry_run {
        println!("[dry-run] Would release {next_version} as {tag_name}");
        return Ok(());
    }

    // 4. Write changelog.
    if config.changelog.enabled {
        info!("→ Writing changelog…");
        let changelog_path = root.join(&config.changelog.file);
        let existing = if changelog_path.exists() {
            std::fs::read_to_string(&changelog_path)?
        } else {
            String::new()
        };
        let section = changelog::render_changelog_section(&commits, &next_version);
        let updated = changelog::prepend_to_file(&existing, &section);
        std::fs::write(&changelog_path, updated)?;
    }

    // 5. Bump Cargo.toml and create release commit.
    info!("→ Bumping Cargo.toml to {next_version}…");
    write_version(&cargo_toml, &next_version)?;
    let release_message = format!("chore: release {tag_name}");
    info!("→ Creating commit {:?}…", release_message);
    backend.new_commit(&release_message)?;

    // 6. Tag the release commit.
    info!("→ Creating tag {tag_name}…");
    backend.create_tag(&tag_name, "@")?;

    // 7. Advance the bookmark.
    info!("→ Moving bookmark {:?} to @…", config.release.bookmark);
    backend.set_bookmark(&config.release.bookmark, "@")?;

    // 8. Export to git and push.
    info!("→ Exporting to git…");
    backend.git_export()?;
    info!("→ Pushing bookmark and tags…");
    backend.git_push(&config.release.bookmark, Some(&tag_name))?;

    // 9. Cargo publish.
    if config.publish.cargo {
        info!("→ Running cargo publish…");
        cargo_publish(root, &config.publish.cargo_flags)?;
    }

    // 10. GitHub release.
    if config.release.github_release {
        info!("→ Creating GitHub release {tag_name}…");
        gh_release_create(&tag_name)?;
    }

    info!("✓ Released {tag_name}");
    Ok(())
}

fn print_next_version(
    backend: &dyn JjBackend,
    config: &Config,
    root: &std::path::Path,
) -> Result<()> {
    let cargo_toml = root.join("Cargo.toml");
    let current = read_version(&cargo_toml)?;
    let since = match latest_version_tag(backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };
    let commits = backend.log_commits(&format!("{since}..@"))?;
    let bump = if let Some(force) = &config.bump.force {
        match force.as_str() {
            "major" => BumpKind::Major,
            "minor" => BumpKind::Minor,
            "patch" => BumpKind::Patch,
            other => bail!("unknown bump.force value {other:?} — must be major/minor/patch"),
        }
    } else {
        compute_bump(&commits)
    };
    let next = apply_bump(&current, bump);
    println!("{next}");
    Ok(())
}

fn cargo_publish(root: &std::path::Path, extra_flags: &[String]) -> Result<()> {
    let status = Command::new("cargo")
        .arg("publish")
        .arg("--allow-dirty")
        .args(extra_flags)
        .current_dir(root)
        .status()
        .context("spawning cargo publish")?;

    if !status.success() {
        bail!("cargo publish failed");
    }
    Ok(())
}

fn gh_release_create(tag: &str) -> Result<()> {
    let status = Command::new("gh")
        .args(["release", "create", tag, "--generate-notes"])
        .status()
        .context("spawning gh release create")?;

    if !status.success() {
        bail!("gh release create failed for tag {tag}");
    }
    Ok(())
}
