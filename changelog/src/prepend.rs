use crate::render::CHANGELOG_HEADER;

// Splits a changelog into its header and the release sections after it.
///
/// The header is everything from the `# Changelog` title (any letter case)
/// up to, but not including, the first `## ` heading, so custom intro text
/// is preserved. Returns `None` if the file doesn't start with that title.
fn split_header(existing: &str) -> Option<(&str, &str)> {
    if !has_changelog_title(existing) {
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
            let body = strip_leading_unreleased(body);
            format!("{header}\n\n{new_section}\n{body}")
        }
        // No changelog title: add the default header and keep the file as is.
        None => format!("{CHANGELOG_HEADER}{new_section}\n{existing}"),
    }
}

/// Whether `existing` starts with a `# Changelog` title (any letter case),
/// ignoring leading whitespace.
#[must_use]
pub fn has_changelog_title(existing: &str) -> bool {
    let first = existing
        .trim_start_matches('\u{feff}') // byte-order mark from some editors
        .trim_start()
        .lines()
        .next()
        .unwrap_or("");
    // "#" followed by a space or nothing; "##" is a section, "#tag" isn't a heading.
    first
        .strip_prefix('#')
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

/// Drop `[Unreleased]` sections at the start of `body`. A new section, whether
/// another preview or an actual release, always supersedes them.
fn strip_leading_unreleased(body: &str) -> &str {
    let mut rest = body.trim_start_matches('\n');
    loop {
        let Some(first) = rest.lines().next() else {
            return rest;
        };
        if !(first.starts_with("## ") && first.contains("[Unreleased]")) {
            return rest;
        }
        // Skip this section: everything up to the next `## ` heading.
        let after = &rest[first.len()..];
        rest = after.find("\n## ").map_or("", |i| &after[i + 1..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECTION: &str = "## [0.2.0] - 2024-06-01\n\n### Added\n- new thing\n";

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
    fn detects_changelog_title() {
        assert!(has_changelog_title("# Changelog\n\nIntro\n"));
        assert!(has_changelog_title("\n\n# CHANGELOG\n"));
        assert!(!has_changelog_title("TBD\n===\n"));
        assert!(!has_changelog_title(""));
    }

    #[test]
    fn repeated_preview_replaces_unreleased() {
        let once = prepend_to_file("", "## [Unreleased]\n\n### Added\n\n- a\n");
        let twice = prepend_to_file(&once, "## [Unreleased]\n\n### Added\n\n- a\n- b\n");
        assert_eq!(twice.matches("## [Unreleased]").count(), 1);
        assert!(twice.contains("- b"));
    }

    #[test]
    fn release_replaces_unreleased_and_keeps_history() {
        let existing = prepend_to_file("", "## [0.1.0] - 2026-01-01\n\n### Added\n\n- old\n");
        let existing = prepend_to_file(&existing, "## [Unreleased]\n\n### Added\n\n- pending\n");
        let released = prepend_to_file(
            &existing,
            "## [0.2.0] - 2026-02-01\n\n### Added\n\n- pending\n",
        );

        assert!(!released.contains("[Unreleased]"));
        assert!(released.find("## [0.2.0]") < released.find("## [0.1.0]"));
        assert!(released.contains("- old"));
    }

    #[test]
    fn detects_any_h1_title() {
        assert!(has_changelog_title("# Changelog\n"));
        assert!(has_changelog_title("# CHANGELOG\n"));
        assert!(has_changelog_title("# Change Log\n"));
        assert!(has_changelog_title("# Release Notes for jj-release\n"));
        assert!(has_changelog_title("\n\n# Changelog\n"));
        assert!(has_changelog_title("\u{feff}# Changelog\n"));
    }

    #[test]
    fn rejects_non_titles() {
        assert!(!has_changelog_title(""));
        assert!(!has_changelog_title("## [0.1.0] - 2026-01-01\n"));
        assert!(!has_changelog_title("#Changelog\n"));
        assert!(!has_changelog_title("TBD\n===\n")); // setext, intentionally unsupported
        assert!(!has_changelog_title("Some text\n# Changelog\n"));
    }
}
