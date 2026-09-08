use std::fmt::Write;

use git_conventional::Commit;
use jiff::Zoned;
use semver::Version;

use crate::commits::CommitInfo;

pub fn render_changelog_section(commits: &[CommitInfo], version: &Version) -> String {
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut fixed = Vec::new();

    for commit in commits {
        let Ok(conv) = Commit::parse(&commit.description) else {
            continue;
        };

        let breaking = conv.breaking();
        let summary = conv.description().to_owned();
        let entry = if breaking {
            format!("**BREAKING** {summary}")
        } else {
            summary
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

pub fn prepend_to_file(existing: &str, new_section: &str) -> String {
    let header = "# Changelog\n\n";
    if existing.is_empty() {
        return format!("{header}{new_section}");
    }
    existing.strip_prefix(header).map_or_else(
        || format!("{header}{new_section}\n{existing}"),
        |rest| format!("{header}{new_section}\n{rest}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_added_section() {
        let commits = vec![
            CommitInfo {
                change_id: "abc".into(),
                description: "feat: add wallpaper cycling".into(),
            },
            CommitInfo {
                change_id: "def".into(),
                description: "feat: add IPC commands".into(),
            },
        ];
        let section = render_changelog_section(&commits, &Version::parse("0.2.0").unwrap());
        assert!(section.contains("## [0.2.0]"));
        assert!(section.contains("### Added"));
        assert!(section.contains("- add wallpaper cycling"));
        assert!(section.contains("- add IPC commands"));
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
        let existing = "# Changelog\n\n## [0.1.0] - 2024-01-01\n\n### Added\n- initial release\n";
        let new_section = "## [0.2.0] - 2024-06-01\n\n### Added\n- new thing\n";
        let result = prepend_to_file(existing, new_section);
        assert!(result.starts_with("# Changelog\n\n## [0.2.0]"));
        assert!(result.contains("## [0.1.0]"));
    }

    #[test]
    fn prepend_creates_header_if_missing() {
        let result = prepend_to_file("", "## [0.1.0] - 2024-01-01\n\n### Added\n- thing\n");
        assert!(result.starts_with("# Changelog\n\n## [0.1.0]"));
    }
}
