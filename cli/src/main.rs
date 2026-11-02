//! jj-release, semantic releases for Jujutsu repositories.

mod commands;

use std::{
    env,
    path::{Path, PathBuf},
    process,
};

use clap::Parser;

use crate::config::Config;
use crate::pipeline::ChangelogOutput;

#[cfg(feature = "forgejo")]
use jj_release_core::forge::ForgejoForge;
#[cfg(feature = "gitea")]
use jj_release_core::forge::GiteaForge;
use jj_release_core::{config, detect, errors, jj, pipeline};
use jj_release_core::{
    errors::{ReleaseError, Result},
    forge::{ForgeBackend, GitHubForge, GitLabForge, NoForge},
    jj::ShellBackend,
    manifest::{CargoManifest, GoManifest, ManifestBackend, NpmManifest},
    pipeline::ReleaseContext,
    publish::{CargoPublish, NoPublish, NpmPublish, PublishBackend},
    workspace::WorkspaceManifest,
};

#[derive(Parser, Debug)]
#[allow(clippy::struct_excessive_bools)]
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

    /// Skip publishing to crates.io even if publish.cargo = true.
    #[arg(long)]
    no_publish: bool,

    /// Force publishing to crates.io even if publish.cargo = false.
    #[arg(long)]
    publish_cargo: bool,
}

#[derive(clap::Subcommand, Debug, Clone)]
enum Subcommand {
    /// Run the full release pipeline (default when no subcommand given).
    Run,
    /// Print the next version that would be released, then exit.
    NextVersion,
    /// Generate changelog for commits since last tag and print to stdout.
    #[command(name = "changelog")]
    Changelog {
        /// Write to this file instead of stdout. Creates or prepends to the file.
        /// Root layout only; use --write for per-member changelogs.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short, long)]
        write: bool,
        /// Generate changelog for all tags, not just since the last one.
        #[arg(long)]
        full: bool,
    },
    /// Push a PR
    Pr,
    /// Auto-detect environment and generate a release.toml.
    Init,
    /// Check that everything is ready for a release without making any changes.
    Validate,
}

fn main() {
    let code = match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            match e {
                ReleaseError::MissingConfig => 2,
                ReleaseError::MissingTool(_) => 3,
                _ => 101,
            }
        }
    };
    process::exit(code);
}

fn run() -> Result<(), errors::ReleaseError> {
    let cli = Cli::parse();
    let root = resolve_repo_root(cli.repo.clone())?;
    let config = load_config(&root, cli.no_publish, cli.publish_cargo)?;

    let backend = ShellBackend::new(&root)?;
    let publisher = build_publisher(&config);
    let manifest = build_manifest(&config);
    let forge = build_forge(&config, &root)?;

    let ctx = ReleaseContext::new(
        &backend,
        manifest.as_ref(),
        forge.as_ref(),
        publisher.as_ref(),
    );

    execute_command(cli, &ctx, &config, &root)
}

fn resolve_repo_root(repo_arg: Option<PathBuf>) -> Result<PathBuf, ReleaseError> {
    if let Some(p) = repo_arg {
        Ok(p)
    } else {
        let cwd = env::current_dir()
            .map_err(|e| ReleaseError::Message(format!("getting current directory: {e}")))?;
        jj::find_repo_root(&cwd)
    }
}

fn load_config(root: &Path, no_publish: bool, publish_cargo: bool) -> Result<Config, ReleaseError> {
    let mut config = config::load(root)?;
    if publish_cargo {
        config.publish.cargo = true;
    } else if no_publish {
        config.publish.cargo = false;
    }

    if !root.join("release.toml").exists() {
        eprintln!(
            "hint: no release.toml found, using defaults. Run `jj-release init` to customize"
        );
    }

    Ok(config)
}

fn build_publisher(config: &Config) -> Box<dyn PublishBackend> {
    match config.manifest_backend.as_str() {
        "npm" => Box::new(NpmPublish),
        _ if config.publish.cargo => Box::new(CargoPublish),
        _ => Box::new(NoPublish),
    }
}

fn build_manifest(config: &Config) -> Box<dyn ManifestBackend> {
    if config.workspace.as_ref().is_some_and(|w| w.enabled) {
        return Box::new(WorkspaceManifest);
    }
    match config.manifest_backend.as_str() {
        "go" => Box::new(GoManifest),
        "npm" => Box::new(NpmManifest),
        _ => Box::new(CargoManifest),
    }
}

#[allow(unused_variables)]
fn build_forge(config: &Config, root: &Path) -> Result<Box<dyn ForgeBackend>, ReleaseError> {
    match config.release.forge.as_str() {
        "github" => {
            if !detect::tool_available("gh") {
                return Err(ReleaseError::MissingTool("gh".to_owned()));
            }
            Ok(Box::new(GitHubForge))
        }
        "gitlab" => {
            if !detect::tool_available("glab") {
                return Err(ReleaseError::MissingTool("glab".to_owned()));
            }
            Ok(Box::new(GitLabForge))
        }
        #[cfg(feature = "forgejo")]
        "forgejo" => {
            let (owner, repo) = get_owner_repo(root);
            Ok(Box::new(ForgejoForge {
                host: config.release.forge_url.clone(),
                token: env::var("FORGEJO_TOKEN").unwrap_or_default(),
                owner,
                repo,
            }))
        }
        #[cfg(feature = "gitea")]
        "gitea" => {
            let (owner, repo) = get_owner_repo(root);
            Ok(Box::new(GiteaForge {
                host: config.release.forge_url.clone(),
                token: env::var("GITEA_TOKEN").unwrap_or_default(),
                owner,
                repo,
            }))
        }
        _ => Ok(Box::new(NoForge)),
    }
}

#[cfg(any(feature = "forgejo", feature = "gitea"))]
fn get_owner_repo(root: &Path) -> (String, String) {
    detect::remote_url(root)
        .as_deref()
        .and_then(detect::parse_remote_owner_repo)
        .unwrap_or_else(|| (String::new(), String::new()))
}

fn execute_command(
    cli: Cli,
    ctx: &ReleaseContext<'_>,
    config: &Config,
    root: &Path,
) -> Result<(), ReleaseError> {
    match cli.command.unwrap_or(Subcommand::Run) {
        Subcommand::Run => commands::release(ctx, config, root, cli.dry_run, cli.quiet),
        Subcommand::NextVersion => pipeline::print_next_version(ctx, config, root),
        Subcommand::Changelog {
            output,
            write,
            full,
        } => {
            let target = match (output.as_deref(), write) {
                (Some(path), _) => ChangelogOutput::Path(path),
                (None, true) => ChangelogOutput::Configured,
                (None, false) => ChangelogOutput::Stdout,
            };
            pipeline::print_changelog(ctx, config, root, target, full)
        }
        Subcommand::Pr => commands::pr(ctx, config, root, cli.quiet),
        Subcommand::Init => commands::init(root),
        Subcommand::Validate => pipeline::validate(ctx, config, root),
    }
}

#[cfg(test)]
mod tests {
    use jj_release_core::config::Config;
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
