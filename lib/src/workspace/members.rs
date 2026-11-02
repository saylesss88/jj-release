use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use cargo_metadata::{DependencyKind, MetadataCommand};
use semver::{Version, VersionReq};

use crate::{
    WorkspaceMember,
    errors::{ReleaseError, Result},
};

/// Workspace members as Cargo resolves them (globs and `exclude` included),
/// with `publish` and `depends_on` filled in from each member's manifest.
///
/// # Errors
///
/// Returns an error if `cargo metadata` fails, e.g. on an invalid manifest.
pub fn parse_workspace_members(cargo_toml: &Path) -> Result<Vec<WorkspaceMember>> {
    let metadata = MetadataCommand::new()
        .manifest_path(cargo_toml)
        .no_deps()
        .exec()?;

    let root = metadata.workspace_root.as_std_path();
    let members = metadata.workspace_packages();
    let names: HashSet<&str> = members.iter().map(|p| p.name.as_str()).collect();

    Ok(members
        .iter()
        .map(|p| {
            let dir = p.manifest_path.parent().map_or(root, |d| d.as_std_path());
            let rel = dir.strip_prefix(root).unwrap_or(dir);
            let path = if rel.as_os_str().is_empty() {
                ".".to_owned() // root package that is also a member
            } else {
                rel.to_string_lossy().replace('\\', "/")
            };

            // Publish order only depends on normal and build deps; dev-deps
            // with a path are stripped by `cargo publish`.
            let depends_on = p
                .dependencies
                .iter()
                .filter(|d| matches!(d.kind, DependencyKind::Normal | DependencyKind::Build))
                .filter(|d| d.path.is_some() && names.contains(d.name.as_str()))
                .map(|d| d.name.clone())
                .collect();

            WorkspaceMember {
                name: p.name.to_string(),
                path,
                // `publish = false` in Cargo.toml shows up as Some(empty list).
                publish: !matches!(&p.publish, Some(registries) if registries.is_empty()),
                depends_on,
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

/// For each member, its version requirements on other workspace members:
/// `dependent -> (dependency -> requirement)`.
///
/// # Errors
///
/// Returns an error if `cargo metadata` fails.
pub fn member_requirements(
    cargo_toml: &Path,
) -> Result<HashMap<String, HashMap<String, VersionReq>>> {
    let metadata = MetadataCommand::new()
        .manifest_path(cargo_toml)
        .no_deps()
        .exec()?;

    let members = metadata.workspace_packages();
    let names: HashSet<&str> = members.iter().map(|p| p.name.as_str()).collect();

    Ok(members
        .iter()
        .map(|p| {
            let reqs = p
                .dependencies
                .iter()
                .filter(|d| matches!(d.kind, DependencyKind::Normal | DependencyKind::Build))
                .filter(|d| names.contains(d.name.as_str()))
                .map(|d| (d.name.clone(), d.req.clone()))
                .collect();
            (p.name.to_string(), reqs)
        })
        .collect())
}
/// The tag prefix for `member`: its own `tag_prefix` if set (even if empty,
/// for bare-version tags like `15.2.0`), otherwise the workspace default.
#[must_use]
pub fn member_prefix<'a>(member: &'a WorkspaceMember, default: &'a str) -> &'a str {
    member.tag_prefix.as_deref().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BumpKind, test_helpers::mock::MockBackend, workspace};
    use jj_release_changelog::CommitInfo;
    use std::fs;

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
        let bumps = workspace::member_bumps(&backend, &members, "v0.1.0", None, "v").unwrap();
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
        assert_eq!(workspace::member_tag_name(&member, &v, "v"), "mylib-v0.5.0");
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
        assert_eq!(workspace::member_tag_name(&member, &v, "v"), "v0.8.0");
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

    #[test]
    fn bump_member_version_with_toml_edit() {
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
            r#"[package]
name = "mylib"
version = "0.3.0"
edition = "2024"
"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("lib/src")).unwrap();
        fs::write(dir.path().join("lib/src/lib.rs"), "").unwrap();

        let new_version = Version::parse("0.4.0").unwrap();
        workspace::bump_member_version(dir.path(), "mylib", &new_version).unwrap();

        let content = fs::read_to_string(dir.path().join("lib/Cargo.toml")).unwrap();
        assert!(content.contains("\"0.4.0\""));
        assert!(!content.contains("\"0.3.0\""));
    }
}
