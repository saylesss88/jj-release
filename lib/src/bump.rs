//! Version bumps in files that aren't project manifests: user-configured
//! regex replacements (README examples, constants) and `flake.nix`.

use std::{fs, path::Path};

use regex::Regex;

use crate::config::Replacement;
use crate::errors::{ReleaseError, Result};

/// Executes regex-based text replacements across arbitrary files (e.g. README.md).
/// Replaces `{{version}}`, `{{crate_name}}`, and `{{date}}` with the provided values.
///
/// # Errors
/// Returns an error if a file cannot be read/written, if the regex is invalid,
/// or if `exactly_one` is true and the search pattern does not match exactly once.
pub fn apply_replacements(
    replacements: &[Replacement],
    crate_name: &str,
    new_version: &str,
    date: &str,
) -> Result<()> {
    for req in replacements {
        let content = fs::read_to_string(&req.file)
            .map_err(|e| ReleaseError::Message(format!("failed to read {}: {e}", req.file)))?;

        let re = Regex::new(&req.search)
            .map_err(|e| ReleaseError::Message(format!("invalid regex '{}': {e}", req.search)))?;

        let replace_template = req
            .replace
            .replace("{{version}}", new_version)
            .replace("{{crate_name}}", crate_name)
            .replace("{{date}}", date);

        let matches = re.find_iter(&content).count();

        if req.exactly_one && matches != 1 {
            return Err(ReleaseError::Message(format!(
                "replacement in {} failed: expected exactly 1 match for '{}', found {}",
                req.file, req.search, matches
            )));
        }

        if matches > 0 {
            let new_content = re.replace_all(&content, replace_template.as_str());
            fs::write(&req.file, new_content.as_ref())
                .map_err(|e| ReleaseError::Message(format!("failed to write {}: {e}", req.file)))?;

            println!("  📝 Updated {} ({} match(es))", req.file, matches);
        }
    }
    Ok(())
}

/// Automatically detects and bumps a standard `version = "..."` declaration inside
/// a `flake.nix` file if it exists at the given root path.
///
/// # Errors
/// Returns an error if the `flake.nix` file cannot be read or written. Does not error
/// if the file doesn't exist, or if the regex fails to find a version string (it just warns).
pub fn bump_flake_nix(root: &Path, new_version: &str) -> Result<()> {
    let flake_path = root.join("flake.nix");

    if !flake_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&flake_path)
        .map_err(|e| ReleaseError::Message(format!("failed to read flake.nix: {e}")))?;

    // Match `version = "x.y.z";` or `version = "x.y.z"` capturing the prefix and suffix
    let re = Regex::new(r#"(?m)^(\s*version\s*=\s*")([^"]+)(".*)$"#)
        .map_err(|e| ReleaseError::Message(format!("flake regex compile error: {e}")))?;

    if !re.is_match(&content) {
        println!(
            "  ⚠️ Found flake.nix, but could not locate a `version = \"...\"` string to update."
        );
        return Ok(());
    }

    let new_content = re.replace(&content, format!("${{1}}{new_version}${{3}}"));

    fs::write(&flake_path, new_content.as_ref())
        .map_err(|e| ReleaseError::Message(format!("failed to write flake.nix: {e}")))?;

    println!("  ❄️  Bumped flake.nix to {new_version}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bump_flake_nix() {
        let dir = tempfile::tempdir().unwrap();
        let flake_path = dir.path().join("flake.nix");

        let initial_flake = r#"
{
  description = "A test flake";
  outputs = { self, nixpkgs }: {
    packages.default = pkgs.buildRustPackage {
      pname = "jj-release";
      version = "0.7.0";
      src = ./.;
    };
  };
}
"#;
        fs::write(&flake_path, initial_flake).unwrap();

        // Perform the bump
        bump_flake_nix(dir.path(), "0.8.0").unwrap();

        // Verify the result
        let updated = fs::read_to_string(&flake_path).unwrap();
        assert!(updated.contains(r#"version = "0.8.0";"#));
        assert!(!updated.contains(r#"version = "0.7.0";"#));
    }

    #[test]
    fn test_bump_flake_nix_missing_file_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        // Don't create a flake.nix, function should return Ok(()) gracefully
        assert!(bump_flake_nix(dir.path(), "1.0.0").is_ok());
    }

    #[test]
    fn test_apply_replacements_basic_and_variables() {
        let dir = tempfile::tempdir().unwrap();
        let readme_path = dir.path().join("README.md");
        fs::write(&readme_path, "Add my_crate version 0.1.0 to your project!").unwrap();

        let replacements = vec![Replacement {
            file: readme_path.to_string_lossy().to_string(),
            search: r"version \d+\.\d+\.\d+".to_string(),
            replace: "version {{version}}".to_string(),
            exactly_one: true,
        }];

        apply_replacements(&replacements, "my_crate", "0.2.0", "2026-09-24").unwrap();

        let updated = fs::read_to_string(&readme_path).unwrap();
        assert_eq!(updated, "Add my_crate version 0.2.0 to your project!");
    }

    #[test]
    fn test_apply_replacements_capture_groups() {
        let dir = tempfile::tempdir().unwrap();
        let code_path = dir.path().join("lib.rs");
        fs::write(&code_path, r#"pub const VERSION: &str = "0.1.0";"#).unwrap();

        let replacements = vec![Replacement {
            file: code_path.to_string_lossy().to_string(),
            // Capture the prefix and suffix, match the version string in the middle
            search: r#"(pub const VERSION: &str = ")[^"]+(";)"#.to_string(),
            // Reconstruct using $1 and $2 capture groups around the new version
            replace: r"${1}{{version}}${2}".to_string(),
            exactly_one: true,
        }];

        apply_replacements(&replacements, "my_crate", "2.0.0", "2026-09-24").unwrap();

        let updated = fs::read_to_string(&code_path).unwrap();
        assert_eq!(updated, r#"pub const VERSION: &str = "2.0.0";"#);
    }

    #[test]
    fn test_apply_replacements_exactly_one_fails() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Setup a file with NO matches
        fs::write(&config_path, "some_other_setting = true").unwrap();

        let replacements = vec![Replacement {
            file: config_path.to_string_lossy().to_string(),
            search: r#"version = ".*""#.to_string(),
            replace: r#"version = "{{version}}""#.to_string(),
            exactly_one: true, // Should fail because match count == 0
        }];

        let result = apply_replacements(&replacements, "my_crate", "1.0.0", "2026-09-24");

        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("expected exactly 1 match"));
    }

    #[test]
    fn test_apply_replacements_exactly_one_false_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Setup a file with MULTIPLE matches
        fs::write(&config_path, "version = \"1.0.0\"\nversion = \"1.0.0\"").unwrap();

        let replacements = vec![Replacement {
            file: config_path.to_string_lossy().to_string(),
            search: r#"version = "1\.0\.0""#.to_string(),
            replace: r#"version = "{{version}}""#.to_string(),
            exactly_one: false, // Will succeed and replace both
        }];

        apply_replacements(&replacements, "my_crate", "2.0.0", "2026-09-24").unwrap();

        let updated = fs::read_to_string(&config_path).unwrap();
        assert_eq!(updated, "version = \"2.0.0\"\nversion = \"2.0.0\"");
    }
}
