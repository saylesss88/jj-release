use std::path::Path;

use anyhow::{bail, Context, Result};
use semver::Version;
use toml_edit::DocumentMut;

use crate::config::WorkspaceMember;
use crate::manifest::ManifestBackend;

pub struct WorkspaceManifest;

impl ManifestBackend for WorkspaceManifest {
    fn read_version(&self, root: &Path) -> Result<Version> {
        let path = root.join("Cargo.toml");
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
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
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut doc: DocumentMut = raw
            .parse()
            .with_context(|| format!("parsing {}", path.display()))?;
        doc["workspace"]["package"]["version"] = toml_edit::value(version.to_string());
        std::fs::write(&path, doc.to_string())
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_members_lib_before_cli() {
        let members = vec![
            WorkspaceMember {
                name: "mycli".into(),
                path: "cli".into(),
                publish: true,
                depends_on: vec!["mylib".into()],
            },
            WorkspaceMember {
                name: "mylib".into(),
                path: "lib".into(),
                publish: true,
                depends_on: vec![],
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
            },
            WorkspaceMember {
                name: "b".into(),
                path: "b".into(),
                publish: true,
                depends_on: vec![],
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
            },
            WorkspaceMember {
                name: "mylib".into(),
                path: "lib".into(),
                publish: true,
                depends_on: vec![],
            },
        ];
        let ordered = ordered_members(&members).unwrap();
        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].name, "mylib");
    }

    #[test]
    fn workspace_manifest_reads_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
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
        std::fs::write(
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
        let updated = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert!(updated.contains("\"0.6.0\""));
        assert!(!updated.contains("\"0.5.32\""));
    }
}
