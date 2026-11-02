use std::{collections::HashMap, path::Path};

use cargo_metadata::MetadataCommand;
use semver::Version;

use crate::{
    BumpKind, CargoManifest, JjBackend, ManifestBackend, WorkspaceMember, commits,
    errors::{ReleaseError, Result},
};

/// Update the version of a workspace member using `cargo set-version`.
///
/// # Errors
///
/// This function will return an error in the following situations:
/// * Spawning the `cargo` command fails (e.g., if Cargo or `cargo-edit` is not installed).
/// * The `cargo set-version` command execution fails with a non-zero exit
pub fn bump_member_version(root: &Path, member_name: &str, version: &Version) -> Result<()> {
    // Find the member's Cargo.toml via cargo_metadata
    let metadata = MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .exec()?;

    let package = metadata
        .packages
        .iter()
        .find(|p| p.name.as_str() == member_name)
        .ok_or_else(|| {
            ReleaseError::Message(format!("member {member_name} not found in workspace"))
        })?;

    let cargo_toml = Path::new(package.manifest_path.as_str());
    let member_dir = cargo_toml.parent().ok_or_else(|| {
        ReleaseError::Message(format!(
            "could not determine parent directory of {}",
            cargo_toml.display()
        ))
    })?;
    CargoManifest.write_version(member_dir, version)?;
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
        let member_since = member_since(backend, member, tag_prefix, since)?;
        let fileset = member_fileset(member, members);
        let member_commits =
            backend.log_commits_for_path(&format!("{member_since}..@"), &fileset)?;
        let bump = commits::resolve_bump(force, &member_commits)?;
        bumps.insert(member.name.clone(), bump);
    }
    Ok(bumps)
}

/// The latest tag for `member`, using its own `tag_prefix` if set, or
/// `fallback` (usually `"root()"`) if it has never been tagged.
///
/// # Errors
///
/// Returns an error if listing tags fails.
pub fn member_since(
    backend: &dyn JjBackend,
    member: &WorkspaceMember,
    tag_prefix: &str,
    fallback: &str,
) -> Result<String> {
    let prefix = member.tag_prefix.as_deref().unwrap_or(tag_prefix);
    Ok(backend
        .list_tags()?
        .into_iter()
        .filter_map(|name| {
            let version = Version::parse(name.strip_prefix(prefix)?).ok()?;
            Some((name, version))
        })
        .max_by(|a, b| a.1.cmp(&b.1))
        .map_or_else(|| fallback.to_owned(), |(name, _)| name))
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

/// The jj fileset covering `member`'s files, minus any other members nested
/// inside it. For a root member at "." this is "everything except the
/// other crates".
#[must_use]
pub fn member_fileset(member: &WorkspaceMember, members: &[WorkspaceMember]) -> String {
    let base = if member.path == "." {
        "all()".to_owned()
    } else {
        format!("root:{:?}", member.path)
    };

    members
        .iter()
        .filter(|other| other.name != member.name)
        .filter(|other| member.path == "." || other.path.starts_with(&format!("{}/", member.path)))
        .fold(base, |acc, other| format!("{acc} ~ root:{:?}", other.path))
}

/// The tag prefix `member` already uses in `tags`, if any.
///
/// Tries common conventions in order: `name-v1.0.0`, `name-1.0.0`,
/// `name@1.0.0`, and for a root package also `v1.0.0` and bare `1.0.0`.
/// A prefix only counts if the rest of the tag parses as semver, so `grep-`
/// doesn't match `grep-cli-0.1.12`.
#[must_use]
pub fn detect_tag_prefix(member: &WorkspaceMember, tags: &[String]) -> Option<String> {
    let name = &member.name;
    let mut candidates = vec![
        format!("{name}-v"),
        format!("{name}-"),
        format!("{name}@v"),
        format!("{name}@"),
    ];
    if member.path == "." {
        candidates.push("v".to_owned());
        candidates.push(String::new());
    }

    candidates.into_iter().find(|prefix| {
        tags.iter().any(|tag| {
            tag.strip_prefix(prefix.as_str())
                .is_some_and(|rest| Version::parse(rest).is_ok())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorkspaceManifest, workspace};
    use std::fs;

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

        let versions = workspace::member_versions(dir.path()).unwrap();
        assert_eq!(versions.get("mylib").unwrap().to_string(), "0.3.0");
        assert_eq!(versions.get("mycli").unwrap().to_string(), "0.5.0");
    }
}
