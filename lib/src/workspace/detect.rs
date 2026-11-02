use std::{fs, path::Path};

use crate::{Versioning, WorkspaceMember, detect, errors::Result, workspace};

pub struct DetectedWorkspace {
    pub members: Vec<WorkspaceMember>,
    pub versioning: Versioning,
}

/// Detects if the given directory is the root of a Cargo workspace.
///
/// # Errors
///
/// Returns an error if:
/// - The root `Cargo.toml` exists but cannot be read (e.g., missing permissions).
/// - The workspace members cannot be successfully parsed from the TOML document.
///
/// # Returns
///
/// Returns `Ok(Some(DetectedWorkspace))` if a workspace is found, or `Ok(None)`
/// if the file is missing or does not contain a workspace definition.
pub fn detect_workspace(root: &Path) -> Result<Option<DetectedWorkspace>> {
    let workspace_toml = root.join("Cargo.toml");
    if !workspace_toml.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&workspace_toml)?;
    if !content.contains("[workspace]") {
        return Ok(None);
    }
    let versioning_str = detect::detect_versioning(&workspace_toml);
    let versioning = if versioning_str == "independent" {
        Versioning::Independent
    } else {
        Versioning::Unified
    };
    let members = workspace::parse_workspace_members(&workspace_toml)?;
    Ok(Some(DetectedWorkspace {
        members,
        versioning,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use crate::{config, workspace};

    #[test]
    fn detects_ripgrep_style_prefixes() {
        let tags: Vec<String> = ["globset-0.4.21", "grep-0.4.1", "grep-cli-0.1.12", "15.2.0"]
            .map(String::from)
            .to_vec();
        let m = |name: &str, path: &str| WorkspaceMember {
            name: name.into(),
            path: path.into(),
            ..Default::default()
        };

        assert_eq!(
            workspace::detect_tag_prefix(&m("globset", "crates/globset"), &tags).as_deref(),
            Some("globset-")
        );
        assert_eq!(
            workspace::detect_tag_prefix(&m("grep", "crates/grep"), &tags).as_deref(),
            Some("grep-")
        );
        assert_eq!(
            workspace::detect_tag_prefix(&m("grep-cli", "crates/cli"), &tags).as_deref(),
            Some("grep-cli-")
        );
        assert_eq!(
            workspace::detect_tag_prefix(&m("ripgrep", "."), &tags).as_deref(),
            Some("")
        );
    }

    #[test]
    fn no_tags_means_no_detection() {
        let m = WorkspaceMember {
            name: "core".into(),
            path: "core".into(),
            ..Default::default()
        };
        assert_eq!(workspace::detect_tag_prefix(&m, &[]), None);
    }

    #[test]
    fn generated_toml_detects_member_tag_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        // Two members with different versions so versioning is detected as independent.
        for (name, version) in [("alpha", "0.4.21"), ("beta", "0.1.12")] {
            let d = dir.path().join("crates").join(name);
            fs::create_dir_all(d.join("src")).unwrap();
            fs::write(
                d.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n"
                ),
            )
            .unwrap();
            fs::write(d.join("src/lib.rs"), "").unwrap();
        }

        let tags = ["alpha-0.4.21", "beta-v0.1.12"].map(String::from);
        let toml_str = config::generate_release_toml(dir.path(), &tags);
        let cfg: Config = toml::from_str(&toml_str).unwrap();

        let members = cfg.workspace.as_ref().unwrap().members.clone();
        let prefix = |name: &str| {
            members
                .iter()
                .find(|m| m.name == name)
                .unwrap()
                .tag_prefix
                .clone()
        };
        assert_eq!(
            prefix("alpha").as_deref(),
            Some("alpha-"),
            "generated:\n{toml_str}"
        );
        assert_eq!(
            prefix("beta").as_deref(),
            Some("beta-v"),
            "generated:\n{toml_str}"
        );
    }
}
