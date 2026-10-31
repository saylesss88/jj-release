//! Render one changelog section from commit messages.
//!
//! Run with: `cargo run -p jj_release_changelog --example basic`

use jj_release_changelog::{ChangelogConfig, CommitInfo, render_changelog_section};
use semver::Version;

fn main() {
    let commits: Vec<CommitInfo> = [
        "feat(cli): add --write flag",
        "fix: handle empty changelog files",
        "feat!: rename render_full_changelog to render_changelog",
        "chore: bump deps", // dropped in strict mode
    ]
    .into_iter()
    .map(CommitInfo::from)
    .collect();

    let section = render_changelog_section(
        &commits,
        Some(&Version::new(0, 11, 0)),
        &ChangelogConfig::default(),
        None,
    );
    print!("{section}");
}
