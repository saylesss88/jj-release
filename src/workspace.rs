//! Workspace support for multi-crate projects.

use std::{collections::HashMap, fs, path::Path, process::Command};

use anyhow::{Context, Result, bail};
use cargo_metadata::MetadataCommand;
use semver::Version;
use toml_edit::DocumentMut;

use crate::{
    commits::{self, BumpKind},
    config::WorkspaceMember,
    jj::JjBackend,
    manifest::ManifestBackend,
};

pub struct WorkspaceManifest;

impl ManifestBackend for WorkspaceManifest {
    fn read_version(&self, root: &Path) -> Result<Version> {
        let path = root.join("Cargo.toml");
        let raw =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let doc: DocumentMut = raw
            .parse()
            .with_context(|| format!("parsing {}", path.display()))?;
        let version_str = doc
            .get("workspace")
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .with_context(|| "missing [workspace.package].version")?;
        Version::parse(version_str).with_context(|| format!("invalid semver {version_str:?}"))
    }

    fn write_version(&self, root: &Path, version: &Version) -> Result<()> {
        let path = root.join("Cargo.toml");
        let raw =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut doc: DocumentMut = raw
            .parse()
            .with_context(|| format!("parsing {}", path.display()))?;
        doc["workspace"]["package"]["version"] = toml_edit::value(version.to_string());
        fs::write(&path, doc.to_string()).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
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
            bail!("circular dependency detected in workspace members");
        }

        let mut progress = false;
        remaining.retain(|member| {
            let deps_satisfied = member
                .depends_on
                .iter()
                .all(|dep| ordered.iter().any(|m| &m.name == dep));

            if deps_satisfied {
                ordered.push(member);
                progress = true;
                false
            } else {
                true
            }
        });

        if !progress {
            bail!("circular dependency detected in workspace members");
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
        .context("spawning cargo set-version — is cargo-edit installed?")?;
    if !status.success() {
        bail!("cargo set-version failed for {member_name}");
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
) -> Result<HashMap<String, BumpKind>> {
    let mut bumps = HashMap::new();
    for member in members {
        let member_commits = backend.log_commits_for_path(&format!("{since}..@"), &member.path)?;
        let bump = commits::resolve_bump(force, &member_commits)?;
        bumps.insert(member.name.clone(), bump);
    }
    Ok(bumps)
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
        let bumps = member_bumps(&backend, &members, "v0.1.0", None).unwrap();
        assert_eq!(*bumps.get("mylib").unwrap(), BumpKind::Minor);
    }
}
