//! Workspace support for multi-crate projects.

use std::{borrow::ToOwned, collections::HashMap, fs, path::Path, process::Command};

use crate::errors::{ReleaseError, Result};
use cargo_metadata::MetadataCommand;
use semver::Version;
use toml_edit::DocumentMut;

use crate::{
    commits::{self, BumpKind},
    config::{Versioning, WorkspaceMember},
    detect,
    jj::JjBackend,
    manifest::ManifestBackend,
};

pub struct WorkspaceManifest;

impl ManifestBackend for WorkspaceManifest {
    fn read_version(&self, root: &Path) -> Result<Version> {
        let path = root.join("Cargo.toml");
        let raw = fs::read_to_string(&path)
            .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;
        let doc: DocumentMut = raw
            .parse()
            .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;
        let version_str = doc
            .get("workspace")
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ReleaseError::Message("missing [workspace.package].version".into()))?;
        Version::parse(version_str)
            .map_err(|e| ReleaseError::Message(format!("invalid semver {version_str:?}: {e}")))
    }

    fn write_version(&self, root: &Path, version: &Version) -> Result<()> {
        let path = root.join("Cargo.toml");

        let raw = fs::read_to_string(&path)
            .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", path.display())))?;

        let mut doc: DocumentMut = raw
            .parse()
            .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", path.display())))?;

        doc["workspace"]["package"]["version"] = toml_edit::value(version.to_string());
        fs::write(&path, doc.to_string())
            .map_err(|e| ReleaseError::Message(format!("writing {}: {e}", path.display())))?;

        Ok(())
    }
}

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
    let members = parse_workspace_members(&workspace_toml)?;
    Ok(Some(DetectedWorkspace {
        members,
        versioning,
    }))
}
/// Parses the `workspace.members` array from a root `Cargo.toml` and resolves their package names.
///
/// # Errors
///
/// Returns an error if the root `Cargo.toml` cannot be read or contains invalid TOML.
pub fn parse_workspace_members(cargo_toml: &Path) -> Result<Vec<WorkspaceMember>> {
    let raw = fs::read_to_string(cargo_toml)
        .map_err(|e| ReleaseError::Message(format!("reading {}: {e}", cargo_toml.display())))?;

    let doc = raw
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| ReleaseError::Message(format!("parsing {}: {e}", cargo_toml.display())))?;

    let Some(members) = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return Ok(vec![]);
    };

    let parent_dir = cargo_toml.parent().unwrap_or_else(|| Path::new(""));

    Ok(members
        .iter()
        .filter_map(|m| m.as_str())
        .map(|path| {
            let member_toml = parent_dir.join(path).join("Cargo.toml");
            let raw = fs::read_to_string(&member_toml).ok();
            let doc = raw
                .as_deref()
                .and_then(|r| r.parse::<toml_edit::DocumentMut>().ok());

            let name = doc
                .as_ref()
                .and_then(|d: &toml_edit::DocumentMut| {
                    d.get("package")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| path.split('/').next_back().unwrap_or(path).to_owned());

            WorkspaceMember {
                name,
                path: path.to_owned(),
                publish: true,
                depends_on: vec![],
                tag_prefix: None,
            }
        })
        .collect())
}

/// Orders publishable workspace members topologically based on their dependencies
/// so they can be published in the correct sequence.
///
/// # Errors
///
/// Returns an error if a circular dependency is detected among the workspace members.
pub fn ordered_members(members: &[WorkspaceMember]) -> Result<Vec<&WorkspaceMember>> {
    let publish: Vec<&WorkspaceMember> = members.iter().filter(|m| m.publish).collect();

    let mut ordered: Vec<&WorkspaceMember> = Vec::new();
    let mut remaining: Vec<&WorkspaceMember> = publish.clone();
    let mut iterations = 0;
    let max = remaining.len() * remaining.len() + 1;

    while !remaining.is_empty() {
        iterations += 1;
        if iterations > max {
            return Err(ReleaseError::Message(
                "circular dependency detected in workspace members".to_string(),
            ));
        }

        let mut progress = false;
        remaining.retain(|member| {
            let deps_satisfied = member.depends_on.iter().all(|dep| {
                // We only care if the dep is scheduled to be published
                let is_publishable = publish.iter().any(|m| &m.name == dep);

                if is_publishable {
                    // If it is, it must have already been processed into `ordered`.
                    ordered.iter().any(|m| &m.name == dep)
                } else {
                    // Not scheduled for publish, so it doesn't block this member.
                    true
                }
            });

            if deps_satisfied {
                ordered.push(member);
                progress = true;
                false // Remove from remaining
            } else {
                true // Keep in remaining
            }
        });

        if !progress {
            return Err(ReleaseError::Message(
                "circular dependency detected in workspace members".to_string(),
            ));
        }
    }

    Ok(ordered)
}

/// Read all workspace member versions via `cargo metadata`.
///
/// # Errors
///
/// This function will return an error in the following situations:
/// * Executing the `cargo metadata` command fails (e.g., if Cargo is not installed
///   or the manifest path is invalid).
/// * Parsing the metadata output or any package version fails.
pub fn member_versions(root: &Path) -> Result<HashMap<String, Version>> {
    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .exec()?;

    let mut versions = HashMap::new();
    for package in &metadata.packages {
        // Only include workspace members, not external dependencies.
        if metadata.workspace_members.contains(&package.id) {
            let version = Version::parse(&package.version.to_string())?;
            versions.insert(package.name.clone().to_string(), version);
        }
    }
    Ok(versions)
}

/// Update the version of a workspace member using `cargo set-version`.
///
/// # Errors
///
/// This function will return an error in the following situations:
/// * Spawning the `cargo` command fails (e.g., if Cargo or `cargo-edit` is not installed).
/// * The `cargo set-version` command execution fails with a non-zero exit
pub fn bump_member_version(root: &Path, member_name: &str, version: &Version) -> Result<()> {
    let status = Command::new("cargo")
        .args(["set-version", "-p", member_name, &version.to_string()])
        .current_dir(root)
        .status()
        .map_err(|_| {
            ReleaseError::Message(
                "spawning cargo set-version, is cargo-edit installed?".to_string(),
            )
        })?;
    if !status.success() {
        return Err(ReleaseError::Message(format!(
            "cargo set-version failed for {member_name}"
        )));
    }
    Ok(())
}

/// Compute the bump kind for each workspace member based on commits
/// that touched its path since the last tag.
///
/// # Errors
///
/// This function will return an error in the following situations:
/// * Querying commit logs for any workspace member path via the backend fails.
/// * Resolving the version bump fails (e.g., if an invalid `force` override string is provided).
pub fn member_bumps(
    backend: &dyn JjBackend,
    members: &[WorkspaceMember],
    since: &str,
    force: Option<&String>,
    tag_prefix: &str,
) -> Result<HashMap<String, BumpKind>> {
    let mut bumps = HashMap::new();
    for member in members {
        // Use member's tag_prefix if set, otherwise workspace default.
        let prefix = member.tag_prefix.as_deref().unwrap_or(tag_prefix);
        let all_tags = backend.list_tags()?;
        let member_since = all_tags
            .iter()
            .filter_map(|name| {
                let stripped = name.strip_prefix(prefix)?;
                let version = Version::parse(stripped).ok()?;
                Some((name.clone(), version))
            })
            .max_by(|a, b| a.1.cmp(&b.1))
            .map_or_else(|| since.to_owned(), |(name, _)| name);
        let member_commits =
            backend.log_commits_for_path(&format!("{member_since}..@"), &member.path)?;
        let bump = commits::resolve_bump(force, &member_commits)?;
        bumps.insert(member.name.clone(), bump);
    }
    Ok(bumps)
}

/// Get the effective tag name for a member version.
#[must_use]
pub fn member_tag_name(
    member: &WorkspaceMember,
    version: &Version,
    default_prefix: &str,
) -> String {
    let prefix = member.tag_prefix.as_deref().unwrap_or(default_prefix);
    format!("{prefix}{version}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commits::CommitInfo;
    use crate::test_helpers::mock::MockBackend;

    #[test]
    fn ordered_members_lib_before_cli() {
        let members = vec![
            WorkspaceMember {
                name: "mycli".into(),
                path: "cli".into(),
                publish: true,
                depends_on: vec!["mylib".into()],
                tag_prefix: None,
            },
            WorkspaceMember {
                name: "mylib".into(),
                path: "lib".into(),
                publish: true,
                depends_on: vec![],
                tag_prefix: None,
            },
        ];
        let ordered = ordered_members(&members).unwrap();
        assert_eq!(ordered[0].name, "mylib");
        assert_eq!(ordered[1].name, "mycli");
    }

    #[test]
    fn ordered_members_no_deps_preserves_order() {
        let members = vec![
            WorkspaceMember {
                name: "a".into(),
                path: "a".into(),
                publish: true,
                depends_on: vec![],
                tag_prefix: None,
            },
            WorkspaceMember {
                name: "b".into(),
                path: "b".into(),
                publish: true,
                depends_on: vec![],
                tag_prefix: None,
            },
        ];
        let ordered = ordered_members(&members).unwrap();
        assert_eq!(ordered[0].name, "a");
        assert_eq!(ordered[1].name, "b");
    }

    #[test]
    fn ordered_members_skips_unpublished() {
        let members = vec![
            WorkspaceMember {
                name: "internal".into(),
                path: "internal".into(),
                publish: false,
                depends_on: vec![],
                tag_prefix: None,
            },
            WorkspaceMember {
                name: "mylib".into(),
                path: "lib".into(),
                publish: true,
                depends_on: vec![],
                tag_prefix: None,
            },
        ];
        let ordered = ordered_members(&members).unwrap();
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].name, "mylib");
    }

    #[test]
    fn workspace_manifest_reads_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace.package]
version = "0.5.32"
edition = "2024"
"#,
        )
        .unwrap();
        let manifest = WorkspaceManifest;
        let v = manifest.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::parse("0.5.32").unwrap());
    }

    #[test]
    fn workspace_manifest_writes_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace.package]
version = "0.5.32"
edition = "2024"
"#,
        )
        .unwrap();
        let manifest = WorkspaceManifest;
        manifest
            .write_version(dir.path(), &Version::parse("0.6.0").unwrap())
            .unwrap();
        let updated = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert!(updated.contains("\"0.6.0\""));
        assert!(!updated.contains("\"0.5.32\""));
    }

    #[test]
    fn independent_versioning_reads_per_member_versions() {
        let dir = tempfile::tempdir().unwrap();

        // Write workspace Cargo.toml
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["lib", "cli"]
resolver = "2"
"#,
        )
        .unwrap();

        // Write lib member
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
        fs::create_dir(dir.path().join("lib/src")).unwrap();
        fs::write(dir.path().join("lib/src/lib.rs"), "").unwrap();

        // Write cli member
        fs::create_dir(dir.path().join("cli")).unwrap();
        fs::write(
            dir.path().join("cli/Cargo.toml"),
            r#"
[package]
name = "mycli"
version = "0.5.0"
edition = "2024"
"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("cli/src")).unwrap();
        fs::write(dir.path().join("cli/src/main.rs"), "fn main() {}").unwrap();

        let versions = member_versions(dir.path()).unwrap();
        assert_eq!(versions.get("mylib").unwrap().to_string(), "0.3.0");
        assert_eq!(versions.get("mycli").unwrap().to_string(), "0.5.0");
    }

    #[test]
    fn bump_member_version_requires_cargo_edit() {
        // If cargo-edit isn't installed this test documents the requirement.
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["lib"]
resolver = "2"
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
        fs::create_dir(dir.path().join("lib/src")).unwrap();
        fs::write(dir.path().join("lib/src/lib.rs"), "").unwrap();

        let new_version = Version::parse("0.4.0").unwrap();
        let result = bump_member_version(dir.path(), "mylib", &new_version);

        if let Err(err) = result {
            // cargo-edit not installed, that's ok, just document it
            let msg = err.to_string();
            assert!(
                msg.contains("cargo set-version") || msg.contains("cargo-edit"),
                "unexpected error: {msg}"
            );
        } else {
            // cargo-edit is installed, verify the version was bumped
            let versions = member_versions(dir.path()).unwrap();
            assert_eq!(versions.get("mylib").unwrap().to_string(), "0.4.0");
        }
    }

    #[test]
    fn member_bumps_computes_per_member() {
        let backend = MockBackend {
            commits: vec![CommitInfo {
                change_id: "abc".into(),
                description: "feat: add thing".into(),
            }],
            ..MockBackend::default()
        };
        let members = vec![WorkspaceMember {
            name: "mylib".into(),
            path: "lib".into(),
            publish: true,
            depends_on: vec![],
            tag_prefix: None,
        }];
        let bumps = member_bumps(&backend, &members, "v0.1.0", None, "v").unwrap();
        assert_eq!(*bumps.get("mylib").unwrap(), BumpKind::Minor);
    }

    #[test]
    fn member_tag_name_uses_member_prefix() {
        let member = WorkspaceMember {
            name: "mylib".into(),
            path: "lib".into(),
            publish: true,
            depends_on: vec![],
            tag_prefix: Some("mylib-v".into()),
        };
        let v = Version::parse("0.5.0").unwrap();
        assert_eq!(member_tag_name(&member, &v, "v"), "mylib-v0.5.0");
    }

    #[test]
    fn member_tag_name_falls_back_to_default_prefix() {
        let member = WorkspaceMember {
            name: "mycli".into(),
            path: "cli".into(),
            publish: true,
            depends_on: vec![],
            tag_prefix: None,
        };
        let v = Version::parse("0.8.0").unwrap();
        assert_eq!(member_tag_name(&member, &v, "v"), "v0.8.0");
    }

    #[test]
    fn ordered_members_ignores_unpublished_dependencies() {
        let members = vec![
            WorkspaceMember {
                name: "mycli".into(),
                path: "cli".into(),
                publish: true,
                depends_on: vec!["internal-core".into()], // Depends on unpublished crate
                tag_prefix: None,
            },
            WorkspaceMember {
                name: "internal-core".into(),
                path: "core".into(),
                publish: false, // NOT published
                depends_on: vec![],
                tag_prefix: None,
            },
        ];

        // This will currently panic with a fake "circular dependency" error
        let ordered = ordered_members(&members).unwrap();

        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].name, "mycli");
    }
}
