//! jj-release, semantic releases for Jujutsu repositories.

use std::env;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Parser;

use jj_release::config::{self, Config};
use jj_release::forge::{ForgeBackend, GitHubForge, GitLabForge, NoForge};
use jj_release::jj::{JjBackend, ShellBackend};
use jj_release::manifest::{CargoManifest, GoManifest, ManifestBackend};
use jj_release::{changelog, commits, commits::BumpKind, jj};

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

    let manifest: Box<dyn ManifestBackend> = match config.manifest_backend.as_str() {
        "go" => Box::new(GoManifest),
        _ => Box::new(CargoManifest), // default to cargo
    };
    // Set up the backend.
    let backend = ShellBackend::new(&root)?;

    let forge: Box<dyn ForgeBackend> = match config.release.forge.as_str() {
        "github" => Box::new(GitHubForge),
        "gitlab" => Box::new(GitLabForge),
        _ => Box::new(NoForge),
    };

    match cli.command.unwrap_or(Subcommand::Run) {
        Subcommand::Run => release_pipeline(
            &backend,
            manifest.as_ref(),
            forge.as_ref(),
            &config,
            &root,
            cli.dry_run,
            cli.quiet,
        ),
        Subcommand::NextVersion => print_next_version(&backend, manifest.as_ref(), &config, &root),
        Subcommand::Changelog => print_changelog(&backend, manifest.as_ref(), &config, &root),
        Subcommand::Pr => release_pr(
            &backend,
            manifest.as_ref(),
            forge.as_ref(),
            &config,
            &root,
            cli.quiet,
        ),
    }
}

fn print_changelog(
    backend: &dyn JjBackend,
    manifest: &dyn ManifestBackend,
    config: &Config,
    root: &std::path::Path,
) -> Result<()> {
    let current = manifest.read_version(root)?;
    let since = match commits::latest_version_tag(backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };
    let commits = backend.log_commits(&format!("{since}..@"))?;
    let next = commits::apply_bump(&current, commits::compute_bump(&commits));
    let section = changelog::render_changelog_section(&commits, &next);
    print!("{section}");
    Ok(())
}

// -- Pipeline --

fn release_pipeline(
    backend: &dyn JjBackend,
    manifest: &dyn ManifestBackend,
    forge: &dyn ForgeBackend,
    config: &Config,
    root: &std::path::Path,
    dry_run: bool,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let since = match commits::latest_version_tag(backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };

    // 1. Detect trigger commit.
    info!(
        "→ Scanning for trigger commit {:?}…",
        config.release.trigger
    );
    let Some(_trigger_id) = commits::find_trigger(backend, &config.release.trigger, &since)? else {
        info!("No trigger commit found. Nothing to release.");
        return Ok(());
    };
    info!("  Found trigger commit.");

    // 2. Read current version.
    let current_version = manifest
        .read_version(root)
        .context("reading current version from Cargo.toml")?;
    info!("  Current version: {current_version}");

    // 3. Fetch commits and compute bump, reuse commits for changelog.
    let commits = backend.log_commits(&format!("{since}..@"))?;
    let bump = if let Some(force) = &config.bump.force {
        match force.as_str() {
            "major" => BumpKind::Major,
            "minor" => BumpKind::Minor,
            "patch" => BumpKind::Patch,
            other => bail!("unknown bump.force value {other:?} — must be major/minor/patch"),
        }
    } else {
        commits::compute_bump(&commits)
    };

    if bump == BumpKind::None {
        info!("No releasable commits found since last tag. Nothing to release.");
        return Ok(());
    }

    let next_version = commits::apply_bump(&current_version, bump);
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
    manifest.write_version(root, &next_version)?;
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

    // 10. Forge release.
    info!("→ Creating forge release {tag_name}…");
    forge.create_release(&tag_name)?;

    info!("✓ Released {tag_name}");
    Ok(())
}

fn release_pr(
    backend: &dyn JjBackend,
    manifest: &dyn ManifestBackend,
    forge: &dyn ForgeBackend,
    config: &Config,
    root: &std::path::Path,
    quiet: bool,
) -> Result<()> {
    macro_rules! info {
        ($($t:tt)*) => { if !quiet { println!($($t)*); } }
    }

    let since = match commits::latest_version_tag(backend, &config.release.tag_prefix)? {
        Some(tag) => tag.name,
        None => "root()".to_owned(),
    };

    // 1. Detect trigger commit.
    info!(
        "→ Scanning for trigger commit {:?}…",
        config.release.trigger
    );
    let Some(_trigger_id) = commits::find_trigger(backend, &config.release.trigger, &since)? else {
        info!("No trigger commit found. Nothing to release.");
        return Ok(());
    };
    info!("  Found trigger commit.");

    // 2. Read current version.
    let current_version = manifest
        .read_version(root)
        .context("reading current version")?;
    info!("  Current version: {current_version}");

    // 3. Compute bump.
    let commits = backend.log_commits(&format!("{since}..@"))?;
    let bump = if let Some(force) = &config.bump.force {
        match force.as_str() {
            "major" => BumpKind::Major,
            "minor" => BumpKind::Minor,
            "patch" => BumpKind::Patch,
            other => bail!("unknown bump.force value {other:?} — must be major/minor/patch"),
        }
    } else {
        commits::compute_bump(&commits)
    };

    if bump == BumpKind::None {
        info!("No releasable commits found since last tag. Nothing to release.");
        return Ok(());
    }

    let next_version = commits::apply_bump(&current_version, bump);
    let tag_name = config.tag_name(&next_version);
    let pr_bookmark = format!("release/{tag_name}");
    info!("  Bump: {bump:?} → {next_version}  (tag: {tag_name})");

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

    // 5. Bump version and create release commit.
    info!("→ Bumping version to {next_version}…");
    manifest.write_version(root, &next_version)?;
    let release_message = format!("chore: release {tag_name}");
    info!("→ Creating commit {:?}…", release_message);
    backend.new_commit(&release_message)?;

    // 6. Push to a release bookmark.
    info!("→ Creating bookmark {pr_bookmark:?}…");
    backend.set_bookmark(&pr_bookmark, "@")?;
    info!("→ Exporting to git…");
    backend.git_export()?;
    info!("→ Pushing {pr_bookmark:?}…");
    backend.git_push(&pr_bookmark, None)?;

    // 7. Open PR.
    info!("→ Opening PR…");
    forge.create_pr(&tag_name, &pr_bookmark, &config.release.bookmark)?;

    info!("✓ PR opened for {tag_name}");
    Ok(())
}

fn print_next_version(
    backend: &dyn JjBackend,
    manifest: &dyn ManifestBackend,
    config: &Config,
    root: &std::path::Path,
) -> Result<()> {
    let current = manifest.read_version(root)?;
    let since = match commits::latest_version_tag(backend, &config.release.tag_prefix)? {
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
        commits::compute_bump(&commits)
    };
    let next = commits::apply_bump(&current, bump);
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
