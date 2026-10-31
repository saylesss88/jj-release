//! Changelog generation following Keep a Changelog format.
use std::{collections::HashMap, fmt::Write};

use git_conventional::Commit;
use jiff::Zoned;
use semver::Version;

const CHANGELOG_HEADER: &str = "# Changelog\n\n\
    All notable changes to this project will be documented in this file.\n\n\
    The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),\n\
    and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n\n";

/// Flat representation of a jj commit, as returned by the backend.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub change_id: String,
    pub description: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[non_exhaustive]
#[allow(clippy::struct_excessive_bools)]
pub struct ChangelogConfig {
    pub enabled: bool,
    pub file: String,
    /// If true, silently drops commits that don't pass strict Conventional Commits parsing.
    /// If false, unformatted commits are placed in an "Other Changes" section.
    pub strict: bool,
    pub prefix_mapping: HashMap<String, String>,
    pub exclude_prefixes: Vec<String>,
    pub emoji_headers: bool,
    pub emoji_mapping: HashMap<String, String>,
    pub link_headers: bool,
}

impl Default for ChangelogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file: "CHANGELOG.md".to_owned(),
            strict: true,
            prefix_mapping: HashMap::new(),
            exclude_prefixes: vec![],
            emoji_headers: false,
            emoji_mapping: HashMap::new(),
            link_headers: false,
        }
    }
}

#[must_use]
pub fn render_changelog_section(
    commits: &[CommitInfo],
    version: Option<&Version>,
    config: &ChangelogConfig,
    compare_url: Option<&str>,
) -> String {
    // Format [Unreleased] without a date, or the specific version with a date
    let heading = version.map_or_else(
        || {
            compare_url.map_or_else(
                || "## [Unreleased]\n".to_owned(),
                |url| format!("## [Unreleased]({url})\n"),
            )
        },
        |v| {
            let date = Zoned::now().date();
            compare_url.map_or_else(
                || format!("## [{v}] - {date}\n"),
                |url| format!("## [{v}]({url}) - {date}\n"),
            )
        },
    );
    let mut out = heading;

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

// Splits a changelog into its header and the release sections after it.
///
/// The header is everything from the `# Changelog` title (any letter case)
/// up to, but not including, the first `## ` heading, so custom intro text
/// is preserved. Returns `None` if the file doesn't start with that title.
fn split_header(existing: &str) -> Option<(&str, &str)> {
    let starts_with_title = existing
        .trim_start()
        .get(..11)
        .is_some_and(|t| t.eq_ignore_ascii_case("# changelog"));
    if !starts_with_title {
        return None;
    }

    let mut offset = 0;
    for line in existing.split_inclusive('\n') {
        if line.starts_with("## ") {
            return Some(existing.split_at(offset));
        }
        offset += line.len();
    }
    // No release sections yet: the whole file is header.
    Some((existing, ""))
}

#[must_use]
pub fn prepend_to_file(existing: &str, new_section: &str) -> String {
    if existing.trim().is_empty() {
        return format!("{CHANGELOG_HEADER}{new_section}");
    }

    match split_header(existing) {
        Some((header, body)) => {
            // Normalise to exactly one blank line before the first section.
            let header = header.trim_end();
            format!("{header}\n\n{new_section}\n{body}")
        }
        // No changelog title: add the default header and keep the file as is.
        None => format!("{CHANGELOG_HEADER}{new_section}\n{existing}"),
    }
}

/// Render a full changelog from all tag ranges, newest first.
/// `sections` is a list of `(tag_name, commits)` pairs in chronological order.
/// `versions` is the corresponding list of versions.
#[must_use]
pub fn render_full_changelog(
    sections: &[(String, Vec<CommitInfo>)],
    versions: &[Option<Version>],
    config: &ChangelogConfig,
    urls: &[Option<String>],
) -> String {
    let mut out = CHANGELOG_HEADER.to_owned();

    // Render newest first.
    for (i, (_, commits)) in sections.iter().enumerate().rev() {
        let version = versions[i].as_ref();
        let url = urls.get(i).and_then(|u| u.as_deref());
        out.push_str(&render_changelog_section(commits, version, config, url));
        out.push('\n');
    }

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

#[must_use]
pub fn compare_url(
    base_url: &str,
    owner_repo: &str,
    prev: &Version,
    next: &Version,
    tag_prefix: &str,
) -> String {
    format!("{base_url}/{owner_repo}/compare/{tag_prefix}{prev}...{tag_prefix}{next}")
}

#[must_use]
pub fn forge_base_url(forge: &str, forge_url: &str) -> Option<String> {
    if !forge_url.is_empty() {
        return Some(forge_url.to_owned());
    }
    match forge {
        "github" => Some("https://github.com".to_owned()),
        "gitlab" => Some("https://gitlab.com".to_owned()),
        "forgejo" | "codeberg" => Some("https://codeberg.org".to_owned()),
        _ => None,
    }
}

#[must_use]
pub fn compare_unreleased_url(
    base_url: &str,
    owner_repo: &str,
    prev: &Version,
    tag_prefix: &str,
) -> String {
    format!("{base_url}/{owner_repo}/compare/{tag_prefix}{prev}...HEAD")
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

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
    fn prepend_inserts_after_header() {
        let new_section = "## [0.2.0] - 2024-06-01\n\n### Added\n- new thing\n";
        let existing =
            format!("{CHANGELOG_HEADER}## [0.1.0] - 2024-05-01\n\n### Added\n- old thing\n");
        let result = prepend_to_file(&existing, new_section);
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
    fn prepend_does_not_duplicate_header() {
        let existing =
            format!("{CHANGELOG_HEADER}## [0.1.0] - 2024-01-01\n\n### Added\n- initial release\n");
        let new_section = "## [0.2.0] - 2024-06-01\n\n### Added\n- new thing\n";
        let result = prepend_to_file(&existing, new_section);
        assert_eq!(result.matches("# Changelog").count(), 1);
    }

    const SECTION: &str = "## [0.2.0] - 2024-06-01\n\n### Added\n- new thing\n";

    #[test]
    fn prepend_keeps_a_custom_header() {
        let existing = "# Changelog\n\nOur own intro, not the Keep a Changelog text.\n\n\
                        ## [0.1.0] - 2024-05-01\n\n### Added\n- old thing\n";
        let result = prepend_to_file(existing, SECTION);
        assert_eq!(result.matches("# Changelog").count(), 1);
        assert!(result.contains("Our own intro"));
        assert!(!result.contains("All notable changes"));
        assert!(result.find("Our own intro") < result.find("## [0.2.0]"));
        assert!(result.find("## [0.2.0]") < result.find("## [0.1.0]"));
    }

    #[test]
    fn prepend_accepts_title_in_any_case_without_blank_line() {
        let existing = "# CHANGELOG\n## [0.1.0] - 2024-05-01\n- old\n";
        let result = prepend_to_file(existing, SECTION);
        assert_eq!(result.to_lowercase().matches("# changelog").count(), 1);
        assert!(result.starts_with("# CHANGELOG\n\n## [0.2.0]"));
        assert!(result.contains("## [0.1.0]"));
    }

    #[test]
    fn prepend_to_header_only_file() {
        let result = prepend_to_file("# Changelog\n\nIntro.\n", SECTION);
        assert_eq!(result, format!("# Changelog\n\nIntro.\n\n{SECTION}\n"));
    }

    #[test]
    fn prepend_does_not_mistake_h3_for_a_release() {
        let existing = "# Changelog\n\n### Notes\nstuff\n\n## [0.1.0]\n- old\n";
        let result = prepend_to_file(existing, SECTION);
        assert!(result.find("### Notes") < result.find("## [0.2.0]"));
    }

    #[test]
    fn prepend_adds_header_to_a_file_without_one() {
        let result = prepend_to_file("## [0.1.0] - 2024-05-01\n- old\n", SECTION);
        assert!(result.starts_with(CHANGELOG_HEADER));
        assert!(result.contains("## [0.1.0]"));
    }

    #[test]
    fn prepend_is_idempotent_for_the_default_header() {
        let once = prepend_to_file("", "## [0.1.0] - 2024-05-01\n- a\n");
        let twice = prepend_to_file(&once, SECTION);
        assert_eq!(twice.matches("# Changelog").count(), 1);
        assert_eq!(twice.matches("All notable changes").count(), 1);
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
            Some(Version::parse("0.1.0").unwrap()),
            Some(Version::parse("0.2.0").unwrap()),
        ];
        let config = ChangelogConfig::default();
        let urls: Vec<Option<String>> = vec![None; sections.len()];
        let result = render_full_changelog(&sections, &versions, &config, &urls);
        assert!(result.contains("## [0.1.0]"));
        assert!(result.contains("## [0.2.0]"));
        // 0.2.0 should come before 0.1.0 (newest first)
        assert!(result.find("## [0.2.0]") < result.find("## [0.1.0]"));
        assert!(result.contains("### Added"));
        assert!(result.contains("### Fixed"));
    }

    #[test]
    fn changelog_strict_false_captures_unformatted_commits() {
        let sections = vec![(
            "v0.1.0".to_string(),
            vec![CommitInfo {
                change_id: "a".into(),
                description: "random work that forgot conventional format".into(),
            }],
        )];
        let versions = vec![Some(Version::parse("0.1.0").unwrap())];
        let config = ChangelogConfig {
            strict: false,
            ..Default::default()
        };
        let urls: Vec<Option<String>> = vec![None; sections.len()];
        // Pass false to disable strict mode
        let result = render_full_changelog(&sections, &versions, &config, &urls);

        assert!(result.contains("## [0.1.0]"));
        assert!(result.contains("### Other Changes"));
        assert!(result.contains("- random work that forgot conventional format"));
    }

    #[test]
    fn compare_url_github() {
        let prev = Version::parse("0.8.0").unwrap();
        let next = Version::parse("0.9.0").unwrap();
        assert_eq!(
            compare_url(
                "https://github.com",
                "saylesss88/jj-release",
                &prev,
                &next,
                "v"
            ),
            "https://github.com/saylesss88/jj-release/compare/v0.8.0...v0.9.0"
        );
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
    fn generates_correct_unreleased_compare_url() {
        let prev = Version::parse("0.10.0").unwrap();

        let url = compare_unreleased_url("https://github.com", "saylesss88/jj-release", &prev, "v");

        assert_eq!(
            url,
            "https://github.com/saylesss88/jj-release/compare/v0.10.0...HEAD"
        );
    }
}
