//! Environment detection for forge, language, and tool availability.

use std::{fs, path::Path, process::Command};

/// Detect the forge from a git remote URL.
#[must_use]
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
#[must_use]
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

/// Get the origin remote URL via git.
#[must_use]
pub fn remote_url(root: &Path) -> Option<String> {
    Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// List filenames present in root (non-recursive, just the top level).
pub fn root_files(root: &Path) -> Vec<String> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Check if a CLI tool is available on PATH.
#[must_use]
pub fn tool_available(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Detect forge from the git remote, falling back to None.
pub fn detect_forge(root: &Path) -> Option<&'static str> {
    remote_url(root).as_deref().and_then(forge_from_url)
}

/// Detect manifest backend from files present in root.
pub fn detect_language(root: &Path) -> Option<&'static str> {
    let files = root_files(root);
    let file_refs: Vec<&str> = files.iter().map(String::as_str).collect();
    language_from_files(&file_refs)
}

/// Detect unified vs. independent versioning in workspace
pub fn detect_versioning(workspace_toml: &Path) -> &'static str {
    let Ok(raw) = fs::read_to_string(workspace_toml) else {
        return "unified";
    };
    let Ok(doc) = raw.parse::<toml_edit::DocumentMut>() else {
        return "unified";
    };
    let Some(members) = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return "unified";
    };
    let Some(workspace_dir) = workspace_toml.parent() else {
        return "unified";
    };

    for member in members.iter().filter_map(|m| m.as_str()) {
        let member_toml = workspace_dir.join(member).join("Cargo.toml");

        let Ok(raw) = fs::read_to_string(&member_toml) else {
            continue;
        };
        let Ok(doc) = raw.parse::<toml_edit::DocumentMut>() else {
            continue;
        };

        // An ordinary string version means this package is independently versioned.
        let has_own_version = doc
            .get("package")
            .and_then(|p| p.get("version"))
            .is_some_and(|version| !version.is_inline_table() && version.as_str().is_some());

        if has_own_version {
            return "independent";
        }
    }

    "unified"
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

    #[test]
    fn detects_unified_versioning() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["lib"]
"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("lib")).unwrap();
        fs::write(
            dir.path().join("lib/Cargo.toml"),
            r#"
[package]
name = "mylib"
version.workspace = true
edition = "2024"
"#,
        )
        .unwrap();
        assert_eq!(detect_versioning(&dir.path().join("Cargo.toml")), "unified");
    }

    #[test]
    fn detects_independent_versioning() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["lib"]
"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("lib")).unwrap();
        fs::write(
            dir.path().join("lib/Cargo.toml"),
            r#"
[package]
name = "mylib"
version = "0.3.0"
edition = "2024"
"#,
        )
        .unwrap();
        assert_eq!(
            detect_versioning(&dir.path().join("Cargo.toml")),
            "independent"
        );
    }

    #[test]
    fn mixed_members_detects_independent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["lib", "cli"]
"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("lib")).unwrap();
        fs::write(
            dir.path().join("lib/Cargo.toml"),
            r#"
[package]
name = "mylib"
version.workspace = true
edition = "2024"
"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("cli")).unwrap();
        fs::write(
            dir.path().join("cli/Cargo.toml"),
            r#"
[package]
name = "mycli"
version = "0.7.0"
edition = "2024"
"#,
        )
        .unwrap();
        assert_eq!(
            detect_versioning(&dir.path().join("Cargo.toml")),
            "independent"
        );
    }
}
