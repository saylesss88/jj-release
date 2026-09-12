/// Detect the forge from a git remote URL.
pub fn forge_from_url(url: &str) -> Option<&'static str> {
    if url.contains("github.com") {
        Some("github")
    } else if url.contains("gitlab.com") {
        Some("gitlab")
    } else if url.contains("codeberg.org") {
        Some("forgejo")
    } else {
        None
    }
}

/// Detect the manifest backend from a list of filenames present in the root.
pub fn language_from_files(files: &[&str]) -> Option<&'static str> {
    // Priority order: cargo > npm > go
    if files.contains(&"Cargo.toml") {
        Some("cargo")
    } else if files.contains(&"package.json") {
        Some("npm")
    } else if files.contains(&"go.mod") {
        Some("go")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_forge_from_github_url() {
        assert_eq!(
            forge_from_url("https://github.com/user/repo.git"),
            Some("github")
        );
        assert_eq!(
            forge_from_url("git@github.com:user/repo.git"),
            Some("github")
        );
    }

    #[test]
    fn detect_forge_from_gitlab_url() {
        assert_eq!(
            forge_from_url("https://gitlab.com/user/repo.git"),
            Some("gitlab")
        );
        assert_eq!(
            forge_from_url("git@gitlab.com:user/repo.git"),
            Some("gitlab")
        );
    }

    #[test]
    fn detect_forge_from_codeberg_url() {
        assert_eq!(
            forge_from_url("https://codeberg.org/user/repo.git"),
            Some("forgejo")
        );
        assert_eq!(
            forge_from_url("git@codeberg.org:user/repo.git"),
            Some("forgejo")
        );
    }

    #[test]
    fn detect_forge_unknown_url() {
        assert_eq!(forge_from_url("https://example.com/user/repo.git"), None);
    }

    #[test]
    fn detect_language_cargo() {
        assert_eq!(language_from_files(&["Cargo.toml"]), Some("cargo"));
    }

    #[test]
    fn detect_language_npm() {
        assert_eq!(language_from_files(&["package.json"]), Some("npm"));
    }

    #[test]
    fn detect_language_go() {
        assert_eq!(language_from_files(&["go.mod"]), Some("go"));
    }

    #[test]
    fn detect_language_cargo_wins_over_npm() {
        assert_eq!(
            language_from_files(&["Cargo.toml", "package.json"]),
            Some("cargo")
        );
    }

    #[test]
    fn detect_language_unknown() {
        assert_eq!(language_from_files(&["main.py"]), None);
    }
}
