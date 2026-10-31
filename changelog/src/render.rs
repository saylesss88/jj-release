use std::collections::HashMap;
use std::fmt::Write;

use git_conventional::Commit;
use jiff::Zoned;
use semver::Version;

use crate::{ChangelogConfig, CommitInfo, Release};

pub const CHANGELOG_HEADER: &str = "# Changelog\n\n\
    All notable changes to this project will be documented in this file.\n\n\
    The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),\n\
    and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n\n";

/// The `## [x.y.z] - date` line, optionally prefixed with a member name.
#[must_use]
pub fn render_heading(
    name: Option<&str>,
    version: Option<&Version>,
    date: Option<&str>,
    compare_url: Option<&str>,
) -> String {
    let label = version.map_or_else(|| "Unreleased".to_owned(), ToString::to_string);
    let linked =
        compare_url.map_or_else(|| format!("[{label}]"), |url| format!("[{label}]({url})"));
    let prefix = name.map_or_else(String::new, |n| format!("{n} "));
    match version {
        Some(_) => {
            let date = date.map_or_else(|| Zoned::now().date().to_string(), ToOwned::to_owned);
            format!("## {prefix}{linked} - {date}\n")
        }
        None => format!("## {prefix}{linked}\n"),
    }
}

/// The grouped `### Added` / `### Fixed` / ... sections, without a heading.
#[must_use]
pub fn render_body(commits: &[CommitInfo], config: &ChangelogConfig) -> String {
    let mut out = String::new();

    // Dynamically group commits by their target header
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();

    for commit in commits {
        if config
            .exclude_prefixes
            .iter()
            .any(|p| commit.description.starts_with(p))
        {
            continue;
        }
        if let Ok(conv) = Commit::parse(&commit.description) {
            let c_type = conv.type_().as_str();

            if config.exclude_prefixes.iter().any(|p| p == c_type) {
                continue;
            }
            let breaking = conv.breaking();
            let scope = conv.scope().map_or_default(|s| format!("**({s})** "));
            let summary = conv.description().to_owned();

            let entry = if breaking {
                format!("**BREAKING** {summary}")
            } else {
                format!("{scope}{summary}")
            };

            let header = if let Some(custom) = config.prefix_mapping.get(c_type) {
                custom.as_str()
            } else {
                match c_type {
                    "feat" => "Added",
                    "fix" => "Fixed",
                    "bug" => "Bug",
                    "refactor" => "Changed",
                    "perf" => "Performance",
                    "docs" => "Documentation",
                    "deprecate" | "deprecated" => "Deprecated",
                    "style" | "chore" | "build" | "ci" | "test" => {
                        // Drop these by default if strict is true.
                        if config.strict {
                            continue;
                        }
                        "Other Changes"
                    }
                    _ if !config.strict => "Other Changes",
                    _ => continue, // Strict drop for completely unknown types
                }
            };
            groups.entry(header.to_string()).or_default().push(entry);
        } else if !config.strict {
            groups
                .entry("Other Changes".to_string())
                .or_default()
                .push(commit.description.clone());
        }
    }

    // Standard "Keep a Changelog" order
    let standard_headers = [
        "Added",
        "Changed",
        "Performance",
        "Deprecated",
        "Removed",
        "Fixed",
        "Bug",
        "Security",
        "Other Changes",
    ];

    for header in standard_headers {
        if let Some(entries) = groups.get(header) {
            let display_header = format_header(header, config);
            let _ = write!(out, "\n### {display_header}\n\n");
            for entry in entries {
                let _ = writeln!(out, "- {entry}");
            }
        }
    }
    // Sort and print any custom headers mapped by the user alphabetically at the bottom
    let mut custom_headers: Vec<_> = groups
        .keys()
        .filter(|k| !standard_headers.contains(&k.as_str()))
        .collect();
    custom_headers.sort();

    for header in custom_headers {
        if let Some(entries) = groups.get(header) {
            let display_header = format_header(header, config);
            let _ = write!(out, "\n### {display_header}\n\n");
            for entry in entries {
                let _ = writeln!(out, "- {entry}");
            }
        }
    }
    out
}

/// Render one release section.
#[must_use]
pub fn render_release(release: &Release, config: &ChangelogConfig) -> String {
    let mut out = render_heading(
        None,
        release.version.as_ref(),
        release.date.as_deref(),
        release.compare_url.as_deref(),
    );
    out.push_str(&render_body(&release.commits, config));
    out
}

/// Render a complete changelog. `releases` are oldest first; output is newest first.
#[must_use]
pub fn render_changelog(releases: &[Release], config: &ChangelogConfig) -> String {
    let mut out = CHANGELOG_HEADER.to_owned();
    for release in releases.iter().rev() {
        out.push_str(&render_release(release, config));
        out.push('\n');
    }
    out
}

#[must_use]
pub fn render_changelog_section(
    commits: &[CommitInfo],
    version: Option<&Version>,
    config: &ChangelogConfig,
    compare_url: Option<&str>,
) -> String {
    let mut out = render_heading(None, version, None, compare_url);
    out.push_str(&render_body(commits, config));
    out
}

fn format_header(header: &str, config: &ChangelogConfig) -> String {
    if !config.emoji_headers {
        return header.to_owned();
    }
    // Check if the user provided a custom emoji for this header
    if let Some(custom_emoji) = config.emoji_mapping.get(header) {
        return format!("{custom_emoji} {header}");
    }
    // Fallback to defaults
    let emoji = match header {
        "Added" => "✨",
        "Changed" => "♻️",
        "Fixed" => "🛠️",
        "Bug" => "🪲",
        "Documentation" => "📚",
        "Performance" => "🚀",
        "Deprecated" => "⚠️",
        "Removed" => "🗑️",
        "Security" => "🔒",
        "Other Changes" => "📦",
        _ => "📌",
    };

    format!("{emoji} {header}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChangelogConfig, ChangelogLayout, CommitInfo, Release};
    use semver::Version;
    use std::collections::HashMap;

    fn test_config() -> ChangelogConfig {
        ChangelogConfig {
            enabled: true,
            emoji_headers: true,
            link_headers: false,
            file: String::new(),
            strict: false,
            exclude_prefixes: vec![],
            prefix_mapping: HashMap::new(),
            emoji_mapping: HashMap::new(),
            layout: ChangelogLayout::default(),
        }
    }

    #[test]
    fn renders_scoped_commit() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "feat(cli): add init subcommand".into(),
        }];
        let config = ChangelogConfig::default();
        let version = Version::parse("0.2.0").unwrap();
        let section = render_changelog_section(&commits, Some(&version), &config, None);
        assert!(section.contains("**(cli)**"));
        assert!(section.contains("add init subcommand"));
    }

    #[test]
    fn renders_fixed_section() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "fix: prevent crash on empty dir".into(),
        }];
        let config = ChangelogConfig::default();
        let version = Version::parse("0.1.1").unwrap();
        let section = render_changelog_section(&commits, Some(&version), &config, None);
        assert!(section.contains("### Fixed"));
        assert!(section.contains("- prevent crash on empty dir"));
    }

    #[test]
    fn omits_empty_sections() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "chore: bump deps".into(),
        }];
        let config = ChangelogConfig::default();
        let version = Version::parse("0.1.1").unwrap();
        let section = render_changelog_section(&commits, Some(&version), &config, None);
        assert!(!section.contains("### Added"));
        assert!(!section.contains("### Fixed"));
    }

    #[test]
    fn breaking_change_gets_note() {
        let commits = vec![CommitInfo {
            change_id: "abc".into(),
            description: "feat!: redesign IPC protocol".into(),
        }];
        let config = ChangelogConfig::default();
        let version = Version::parse("1.0.0").unwrap();
        let section = render_changelog_section(&commits, Some(&version), &config, None);
        assert!(section.contains("### Added"));
        assert!(section.contains("**BREAKING**"));
    }

    #[test]
    fn full_changelog_generates_all_sections() {
        let releases = vec![
            Release {
                version: Some(Version::new(0, 1, 0)),
                commits: vec!["feat: initial implementation".into()],
                ..Default::default()
            },
            Release {
                version: Some(Version::new(0, 2, 0)),
                commits: vec!["fix: handle edge case".into()],
                ..Default::default()
            },
        ];
        let result = render_changelog(&releases, &ChangelogConfig::default());
        assert!(result.find("## [0.2.0]") < result.find("## [0.1.0]"));
        assert!(result.contains("### Added"));
        assert!(result.contains("### Fixed"));
    }

    #[test]
    fn changelog_strict_false_captures_unformatted_commits() {
        let releases = vec![Release {
            version: Some(Version::new(0, 1, 0)),
            commits: vec!["random work that forgot conventional format".into()],
            ..Default::default()
        }];
        let result = render_changelog(&releases, &ChangelogConfig::default().strict(false));
        assert!(result.contains("### Other Changes"));
        assert!(result.contains("- random work that forgot conventional format"));
    }

    #[test]
    fn render_section_with_compare_url() {
        let commits = vec![CommitInfo {
            change_id: "1".into(),
            description: "feat: add thing".into(),
        }];
        let version = Version::parse("0.9.0").unwrap();
        let url = "https://github.com/saylesss88/jj-release/compare/v0.8.0...v0.9.0";
        let out = render_changelog_section(
            &commits,
            Some(&version),
            &ChangelogConfig::default(),
            Some(url),
        );
        assert!(out.contains(&format!("## [0.9.0]({url})")));
    }

    #[test]
    fn render_section_without_compare_url() {
        let commits = vec![CommitInfo {
            change_id: "1".into(),
            description: "feat: add thing".into(),
        }];
        let version = Version::parse("0.9.0").unwrap();
        let out =
            render_changelog_section(&commits, Some(&version), &ChangelogConfig::default(), None);
        assert!(out.contains("## [0.9.0] -"));
        assert!(!out.contains("http"));
    }

    #[test]
    fn renders_unreleased_header_without_url() {
        let commits = vec![CommitInfo {
            change_id: "abc12345".into(),
            description: "feat(core): add shiny new feature".into(),
        }];

        let out = render_changelog_section(&commits, None, &test_config(), None);

        // Should start with [Unreleased] and contain no dates or links
        assert!(out.starts_with("## [Unreleased]\n"));
        // Verify it still parsed the commit under the correct emoji header
        assert!(out.contains("### ✨ Added\n"));
        assert!(out.contains("- **(core)** add shiny new feature\n"));
    }

    #[test]
    fn renders_unreleased_header_with_compare_url() {
        let commits = vec![];
        let url = "https://github.com/saylesss88/jj-release/compare/v0.10.0...HEAD";

        let out = render_changelog_section(&commits, None, &test_config(), Some(url));

        // Should inject the markdown link pointing to HEAD
        assert!(out.starts_with(
            "## [Unreleased](https://github.com/saylesss88/jj-release/compare/v0.10.0...HEAD)\n"
        ));
    }

    #[test]
    fn heading_with_member_name() {
        let v = Version::parse("0.4.0").unwrap();
        let h = render_heading(Some("jj_release_core"), Some(&v), None, None);
        assert!(h.starts_with("## jj_release_core [0.4.0] - "));
    }

    #[test]
    fn heading_without_name_matches_old_format() {
        let v = Version::parse("0.4.0").unwrap();
        assert!(render_heading(None, Some(&v), None, None).starts_with("## [0.4.0] - "));
        assert_eq!(render_heading(None, None, None, None), "## [Unreleased]\n");
    }

    #[test]
    fn body_has_no_heading() {
        let commits = vec![CommitInfo {
            change_id: "a".into(),
            description: "feat: thing".into(),
        }];
        let body = render_body(&commits, &ChangelogConfig::default());
        assert!(!body.contains("## ["));
        assert!(body.contains("### Added"));
    }
}
