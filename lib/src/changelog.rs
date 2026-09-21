//! Changelog generation following Keep a Changelog format.

use std::fmt::Write;

use git_conventional::Commit;
use jiff::Zoned;
use semver::Version;

use crate::commits::CommitInfo;

#[must_use]
pub fn render_changelog_section(commits: &[CommitInfo], version: &Version) -> String {
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut fixed = Vec::new();

    for commit in commits {
        let Ok(conv) = Commit::parse(&commit.description) else {
            continue;
        };

        let breaking = conv.breaking();
        let scope = conv
            .scope()
            .map(|s| format!("**({s})**"))
            .unwrap_or_default();
        let summary = conv.description().to_owned();
        let entry = if breaking {
            format!("**BREAKING** {summary}")
        } else {
            format!("{scope}{summary}")
        };

        match conv.type_().as_str() {
            "feat" => added.push(entry),
            "fix" => fixed.push(entry),
            "perf" | "refactor" => changed.push(entry),
            _ => {}
        }
    }

    let date = Zoned::now().strftime("%Y-%m-%d");
    let mut out = format!("## [{version}] - {date}\n");

    for (heading, entries) in [("Added", &added), ("Changed", &changed), ("Fixed", &fixed)] {
        if entries.is_empty() {
            continue;
        }
        let _ = write!(out, "\n### {heading}\n");
        for entry in entries {
            let _ = writeln!(out, "- {entry}");
        }
    }

    out
}

#[must_use]
pub fn prepend_to_file(existing: &str, new_section: &str) -> String {
    let header = "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),\nand this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n\n";
    if existing.is_empty() {
        return format!("{header}{new_section}");
    }

    existing.strip_prefix("# Changelog\n\n").map_or_else(
        || format!("{header}{new_section}\n{existing}"),
        |rest| format!("{header}{new_section}\n{rest}"),
    )
}

/// Render a full changelog from all tag ranges, newest first.
/// `sections` is a list of `(tag_name, commits)` pairs in chronological order.
/// `versions` is the corresponding list of versions.
#[must_use]
pub fn render_full_changelog(
    sections: &[(String, Vec<CommitInfo>)],
    versions: &[Version],
) -> String {
    let header = "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),\nand this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n\n";

    let mut out = header.to_owned();

    // Render newest first.
    for (i, (_, commits)) in sections.iter().enumerate().rev() {
        let version = &versions[i];
        out.push_str(&render_changelog_section(commits, version));
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_scoped_commit() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "feat(cli): add init subcommand".into(),
        }];
        let section = render_changelog_section(&commits, &Version::parse("0.2.0").unwrap());
        assert!(section.contains("**(cli)**"));
        assert!(section.contains("add init subcommand"));
    }

    #[test]
    fn renders_fixed_section() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "fix: prevent crash on empty dir".into(),
        }];
        let section = render_changelog_section(&commits, &Version::parse("0.1.1").unwrap());
        assert!(section.contains("### Fixed"));
        assert!(section.contains("- prevent crash on empty dir"));
    }

    #[test]
    fn omits_empty_sections() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "chore: bump deps".into(),
        }];
        let section = render_changelog_section(&commits, &Version::parse("0.1.1").unwrap());
        assert!(!section.contains("### Added"));
        assert!(!section.contains("### Fixed"));
    }

    #[test]
    fn breaking_change_gets_note() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "feat!: redesign IPC protocol".into(),
        }];
        let section = render_changelog_section(&commits, &Version::parse("1.0.0").unwrap());
        assert!(section.contains("### Added"));
        assert!(section.contains("**BREAKING**"));
    }

    #[test]
    fn prepend_inserts_after_header() {
        let existing = "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),\nand this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n\n## [0.1.0] - 2024-01-01\n\n### Added\n- initial release\n";
        let new_section = "## [0.2.0] - 2024-06-01\n\n### Added\n- new thing\n";
        let result = prepend_to_file(existing, new_section);
        assert!(result.contains("## [0.2.0]"));
        assert!(result.contains("## [0.1.0]"));
        // New section should come before old.
        assert!(result.find("## [0.2.0]") < result.find("## [0.1.0]"));
    }

    #[test]
    fn prepend_creates_header_if_missing() {
        let result = prepend_to_file("", "## [0.1.0] - 2024-01-01\n\n### Added\n- thing\n");
        assert!(result.starts_with("# Changelog\n\nAll notable"));
        assert!(result.contains("Keep a Changelog"));
    }

    #[test]
    fn full_changelog_generates_all_sections() {
        let sections = vec![
            (
                "v0.1.0".to_string(),
                vec![CommitInfo {
                    change_id: "a".into(),
                    description: "feat: initial implementation".into(),
                }],
            ),
            (
                "v0.2.0".to_string(),
                vec![CommitInfo {
                    change_id: "b".into(),
                    description: "fix: handle edge case".into(),
                }],
            ),
        ];
        let versions = vec![
            semver::Version::parse("0.1.0").unwrap(),
            semver::Version::parse("0.2.0").unwrap(),
        ];
        let result = render_full_changelog(&sections, &versions);
        assert!(result.contains("## [0.1.0]"));
        assert!(result.contains("## [0.2.0]"));
        // 0.2.0 should come before 0.1.0 (newest first)
        assert!(result.find("## [0.2.0]") < result.find("## [0.1.0]"));
        assert!(result.contains("### Added"));
        assert!(result.contains("### Fixed"));
    }
}
