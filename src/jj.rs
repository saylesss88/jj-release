//! Thin abstraction over jj operations.
//!
//! All jj interaction goes through [`JjBackend`]. The [`ShellBackend`] impl
//! shells out to the `jj` binary. A future `LibBackend` could use jj-lib
//! directly without changing anything upstream of this module.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::commits::CommitInfo;

/// Everything jj-release needs from the VCS layer.
pub trait JjBackend {
    /// Return `(change_id, description)` pairs for every commit matched by
    /// `revset`, newest-first.
    ///
    /// # Errors
    ///
    /// Returns an error if the `revset` expression is syntactically invalid or
    /// if the underlying command execution fails.
    fn log_commits(&self, revset: &str) -> Result<Vec<CommitInfo>>;

    /// Create a new commit on top of `@` with the given message.
    /// Returns the new commit's change id.
    ///
    /// # Errors
    ///
    /// Returns an error if there are unresolved conflicts or if the commit
    /// creation fails.
    fn new_commit(&self, message: &str) -> Result<String>;

    /// Create (or move) a tag to the given revision.
    /// # Errors
    ///
    /// Returns an error if the target revision does not exist or if tag
    /// creation fails.
    fn create_tag(&self, tag: &str, revision: &str) -> Result<()>;

    /// Move a bookmark to the given revision.
    ///
    /// # Errors
    ///
    /// Returns an error if the bookmark or revision can't be resolved, or
    /// if the update fails.
    fn set_bookmark(&self, name: &str, revision: &str) -> Result<()>;

    /// Push bookmark(s) and tags to `origin`.
    ///
    /// # Errors
    ///
    /// Returns an error if network operations fail, authentication fails,
    /// or the remote rejects the push.
    fn git_push(&self, bookmark: &str, tag: Option<&str>) -> Result<()>;

    /// Export jj state to the colocated git repo (needed before raw git ops).
    ///
    /// # Errors
    ///
    /// Returns an error if the export fails due to workspace state or
    /// command execution failure.
    fn git_export(&self) -> Result<()>;

    /// List all local tag names.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying command fails to retrieve tags.
    fn list_tags(&self) -> Result<Vec<String>>;
}

// Shell implementation

/// Shells out to the `jj` binary found on `$PATH`.
pub struct ShellBackend {
    /// Repo root (where `.jj/` lives).
    root: PathBuf,
    /// Path to the `jj` binary.
    jj_bin: PathBuf,
}

impl ShellBackend {
    /// Creates a new backend instance rooted at the specified path,
    /// locating the jj binary on the system PATH.
    ///
    /// # Errors
    ///
    /// Returns an error if the jj executable is not found on the system PATH.
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let jj_bin =
            which::which("jj").context("jj binary not found on PATH: is Jujutsu installed?")?;
        Ok(Self {
            root: root.as_ref().to_path_buf(),
            jj_bin,
        })
    }

    /// Run `jj <args>` from the repo root, return stdout as a String.
    fn run(&self, args: &[&str]) -> Result<String> {
        let out = Command::new(&self.jj_bin)
            .args(args)
            // Disable the interactive pager and colour so output is parseable.
            .env("JJ_CONFIG", "") // don't let user config interfere
            .arg("--no-pager")
            .current_dir(&self.root)
            .output()
            .with_context(|| format!("failed to spawn jj {}", args.join(" ")))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            bail!("jj {} failed:\n{stderr}", args.join(" "));
        }

        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run `jj <args>` and discard stdout (for mutation commands).
    fn run_silent(&self, args: &[&str]) -> Result<()> {
        self.run(args).map(|_| ())
    }
}

impl JjBackend for ShellBackend {
    fn log_commits(&self, revset: &str) -> Result<Vec<CommitInfo>> {
        // Use a separator that won't appear in commit messages.
        // Format: <change_id>\x1f<description>\x1e
        let raw = self.run(&[
            "log",
            "--no-graph",
            "-r",
            revset,
            "-T",
            r#"change_id ++ "\x1f" ++ description ++ "\x1e""#,
        ])?;

        let mut commits = Vec::new();
        for record in raw.split('\x1e') {
            let record = record.trim();
            if record.is_empty() {
                continue;
            }
            let (change_id, description) = record
                .split_once('\x1f')
                .with_context(|| format!("unexpected log format: {record:?}"))?;
            commits.push(CommitInfo {
                change_id: change_id.trim().to_owned(),
                description: description.trim().to_owned(),
            });
        }

        Ok(commits)
    }

    fn new_commit(&self, message: &str) -> Result<String> {
        // `jj describe` snapshots whatever is currently on disk into @ and
        // sets the message, so edits made before this call (e.g. Cargo.toml)
        // are captured in the commit rather than left as working-copy changes.
        self.run_silent(&["describe", "-m", message])?;

        let id = self.run(&["log", "--no-graph", "-r", "@", "-T", r#"change_id ++ "\n""#])?;
        Ok(id.trim().to_owned())
    }

    fn create_tag(&self, tag: &str, revision: &str) -> Result<()> {
        self.run_silent(&["tag", "set", tag, "-r", revision, "--allow-move"])
    }

    fn set_bookmark(&self, name: &str, revision: &str) -> Result<()> {
        self.run_silent(&["bookmark", "set", name, "--revision", revision])
    }

    fn git_push(&self, bookmark: &str, tag: Option<&str>) -> Result<()> {
        // Push the bookmark first.
        self.run_silent(&["git", "push", "--bookmark", bookmark])?;
        // Push the tag by name, --tag is mutually exclusive with --bookmark.
        if let Some(tag) = tag {
            self.run_silent(&["git", "push", "--tag", tag])?;
        }
        Ok(())
    }

    fn git_export(&self) -> Result<()> {
        self.run_silent(&["git", "export"])
    }

    fn list_tags(&self) -> Result<Vec<String>> {
        let raw = self.run(&["tag", "list", "--template", r#"name ++ "\n""#])?;
        Ok(raw
            .lines()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect())
    }
}

// -- Helpers --

/// Locate the repo root by walking up from `start` until we find `.jj/`.
///
/// # Errors
///
/// Returns an error if no .jj/ directory is found between the starting path
/// and the filesystem root.
pub fn find_repo_root(start: &Path) -> Result<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".jj").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!(
                "not inside a jj repository (no .jj/ found from {})",
                start.display()
            );
        }
    }
}
