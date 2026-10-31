//! Emoji headers, custom prefix mappings, and non-strict mode.
//!
//! Run with: `cargo run -p jj_release_changelog --example custom_config`

use jj_release_changelog::{ChangelogConfig, CommitInfo, render_changelog_section};
use semver::Version;

fn main() {
    let config = ChangelogConfig::default()
        .emoji_headers(true)
        .strict(false) // keep non-conventional commits under "Other Changes"
        .map_prefix("security", "Security")
        .exclude_prefix("ci");

    let commits: Vec<CommitInfo> = [
        "feat: add per-member changelogs",
        "security: bump vulnerable dependency",
        "ci: fix release workflow", // excluded
        "updated the readme",       // non-conventional, kept because strict = false
    ]
    .into_iter()
    .map(CommitInfo::from)
    .collect();

    print!(
        "{}",
        render_changelog_section(&commits, Some(&Version::new(1, 0, 0)), &config, None)
    );
}
