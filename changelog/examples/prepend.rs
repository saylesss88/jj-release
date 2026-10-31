//! Prepend a new section to an existing changelog, keeping its header.
//!
//! Run with: `cargo run -p jj_release_changelog --example prepend`

use jj_release_changelog::{ChangelogConfig, CommitInfo, prepend_to_file, render_changelog_section};
use semver::Version;

fn main() {
    let existing = "# Changelog\n\nOur own intro text is preserved.\n\n\
                    ## [0.1.0] - 2026-01-05\n\n### Added\n\n- initial release\n";

    let section = render_changelog_section(
        &[CommitInfo::from("fix: off-by-one in pagination")],
        Some(&Version::new(0, 1, 1)),
        &ChangelogConfig::default(),
        None,
    );

    print!("{}", prepend_to_file(existing, &section));
}
