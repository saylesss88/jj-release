//! Render a complete changelog from several releases, with dates and links.
//!
//! Run with: `cargo run -p jj_release_changelog --example history`

use jj_release_changelog::{ChangelogConfig, Release, compare_url, render_changelog};
use semver::Version;

fn main() {
    let base = "https://github.com";
    let repo = "owner/project";
    let v1 = Version::new(0, 1, 0);
    let v2 = Version::new(0, 2, 0);

    let releases = vec![
        Release {
            name: Some(String::from("cli")),
            version: Some(v1.clone()),
            date: Some("2026-01-05".into()),
            commits: vec!["feat: initial release".into()],
            ..Default::default()
        },
        Release {
            name: Some(String::from("cli")),
            version: Some(v2.clone()),
            date: Some("2026-03-10".into()),
            compare_url: Some(compare_url(base, repo, &v1, &v2, "v")),
            commits: vec!["feat: add widgets".into(), "fix: widget overflow".into()],
        },
        Release {
            name: Some(String::from("cli")),
            version: None, // unreleased work since v0.2.0
            commits: vec!["feat: something in progress".into()],
            ..Default::default()
        },
    ];

    print!(
        "{}",
        render_changelog(&releases, &ChangelogConfig::default())
    );
}
