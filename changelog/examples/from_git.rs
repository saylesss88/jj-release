//! Build a changelog section from `git log`, no jj required.
//!
//! Run with: `cargo run -p jj_release_changelog --example from_git -- v0.1.0`

use std::{env, process::Command};

use jj_release_changelog::{ChangelogConfig, CommitInfo, render_changelog_section};

fn main() {
    let since = env::args().nth(1);
    let range = since.map_or_else(|| "HEAD".to_owned(), |tag| format!("{tag}..HEAD"));

    let output = Command::new("git")
        .args(["log", "--format=%H%x00%s", &range])
        .output()
        .expect("failed to run git");
    if !output.status.success() {
        eprintln!("git log failed: {}", String::from_utf8_lossy(&output.stderr));
        return;
    }

    let commits: Vec<CommitInfo> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('\0'))
        .map(|(hash, subject)| CommitInfo {
            change_id: hash.to_owned(),
            description: subject.to_owned(),
        })
        .collect();

    // None renders [Unreleased].
    print!(
        "{}",
        render_changelog_section(&commits, None, &ChangelogConfig::default(), None)
    );
}
