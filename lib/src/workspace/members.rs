use std::{collections::HashMap, fs, path::Path};

use cargo_metadata::MetadataCommand;
use semver::Version;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BumpKind, test_helpers::mock::MockBackend, workspace};
    use jj_release_changelog::CommitInfo;

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
