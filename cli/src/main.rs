//! jj-release, semantic releases for Jujutsu repositories.

mod commands;

use std::{env, path::PathBuf, process};

use clap::Parser;

use jj_release_core::{config, detect, errors, jj, pipeline};
use jj_release_core::{
    errors::{ReleaseError, Result},
    forge::{ForgeBackend, ForgejoForge, GitHubForge, GitLabForge, GiteaForge, NoForge},
    jj::ShellBackend,
    manifest::{CargoManifest, GoManifest, ManifestBackend, NpmManifest},
    pipeline::ReleaseContext,
    publish::{CargoPublish, NoPublish, NpmPublish, PublishBackend},
    workspace::WorkspaceManifest,
};

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

    /// Skip publishing to crates.io even if publish.cargo = true.
    #[arg(long)]
    no_publish: bool,
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
        // Generate changelog for all tags, not just since the last one.
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

    // Resolve repo root.
    let cwd = env::current_dir()
        .map_err(|e| ReleaseError::Message(format!("getting current directory: {e}")))?;
    let root = match &cli.repo {
        Some(p) => p.clone(),
        None => jj::find_repo_root(&cwd)?,
    };

    // Load config (falls back to defaults if release.toml absent).
    let mut config = config::load(&root)?;

    // Override publish if --no-publish flag is set.
    if cli.no_publish {
        config.publish.cargo = false;
    }

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
                return Err(ReleaseError::MissingTool("gh".to_owned()));
            }
            Box::new(GitHubForge)
        }
        "gitlab" => {
            if !detect::tool_available("glab") {
                return Err(ReleaseError::MissingTool("glab".to_owned()));
            }

            Box::new(GitLabForge)
        }
        "forgejo" => {
            let (owner, repo) = detect::remote_url(&root)
                .as_deref()
                .and_then(detect::parse_remote_owner_repo)
                .unwrap_or_else(|| (String::new(), String::new()));
            Box::new(ForgejoForge {
                host: config.release.forge_url.clone(),
                token: env::var("FORGEJO_TOKEN").unwrap_or_default(),
                owner,
                repo,
            })
        }
        "gitea" => {
            let (owner, repo) = detect::remote_url(&root)
                .as_deref()
                .and_then(detect::parse_remote_owner_repo)
                .unwrap_or_else(|| (String::new(), String::new()));
            Box::new(GiteaForge {
                host: config.release.forge_url.clone(),
                token: env::var("GITEA_TOKEN").unwrap_or_default(),
                owner,
                repo,
            })
        }

        _ => Box::new(NoForge),
    };

    let ctx = ReleaseContext::new(
        &backend,
        manifest.as_ref(),
        forge.as_ref(),
        publisher.as_ref(),
    );

    match cli.command.unwrap_or(Subcommand::Run) {
        Subcommand::Run => Ok(commands::release(
            &ctx,
            &config,
            &root,
            cli.dry_run,
            cli.quiet,
        )?),
        Subcommand::NextVersion => Ok(pipeline::print_next_version(&ctx, &config, &root)?),
        Subcommand::Changelog { output, full } => Ok(pipeline::print_changelog(
            &ctx,
            &config,
            &root,
            output.as_deref(),
            full,
        )?),
        Subcommand::Pr => Ok(commands::pr(&ctx, &config, &root, cli.quiet)?),
        Subcommand::Init => Ok(commands::init(&root)?),
        Subcommand::Validate => Ok(pipeline::validate(&ctx, &config, &root)?),
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
