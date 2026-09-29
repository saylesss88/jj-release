//! Publish a new version to Fedora COPR.
//!
//! Mirrors the manual workflow: copy the spec into a temp dir and set its
//! `Version:`, download the published `.crate` (Source0), extract it and run
//! `cargo vendor` to build `vendor.tar.xz` (Source1), build an SRPM with
//! `rpmbuild -bs`, then submit it with `copr-cli build`.
//!
//! The spec in your repo is never modified; only the temp copy is.
//!
//! Requires an RPM-based host (Fedora) with `curl`, `tar`, `xz`, `cargo`,
//! `rpmbuild`, `cargo-rpm-macros` and a configured `copr-cli`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use regex::{Captures, Regex};
use semver::Version;

use super::{Check, download, require_tools};
use crate::config::CoprConfig;
use crate::errors::{ReleaseError, Result};

/// Build an SRPM for `version` and submit it to COPR.
///
/// With `dry_run`, builds the SRPM but doesn't submit it.
///
/// # Errors
/// Returns an error if the spec is missing required fields, the version is a
/// pre-release, or any download, vendoring, `rpmbuild` or `copr-cli` step fails.
pub fn publish(cfg: &CoprConfig, root: &Path, version: &Version, dry_run: bool) -> Result<()> {
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(msg(format!(
            "COPR publishing doesn't support pre-release or build-metadata versions yet ({version})"
        )));
    }
    let new = version.to_string();

    let spec_src = root.join(&cfg.spec);
    let spec = read(&spec_src)?;
    let crate_name = spec_crate_name(&spec)?.to_owned();
    check_vendor_source(&spec)?;
    let spec = set_spec_version(&spec, &new)?;

    let tmp = tempfile::tempdir().map_err(|e| msg(format!("creating temp dir: {e}")))?;
    let work = tmp.path();

    let spec_name = spec_src
        .file_name()
        .ok_or_else(|| msg(format!("invalid spec path {}", spec_src.display())))?;
    let spec_path = work.join(spec_name);
    write(&spec_path, spec.as_bytes())?;

    // Source0: the published crate.
    let crate_file = format!("{crate_name}-{new}.crate");
    let url = format!("https://static.crates.io/crates/{crate_name}/{crate_file}");
    write(&work.join(&crate_file), &download(&url)?)?;

    // Source1: vendored dependencies.
    run(
        Command::new("tar")
            .args(["-xf", &crate_file])
            .current_dir(work),
        "extracting crate",
    )?;
    let crate_dir = work.join(format!("{crate_name}-{new}"));
    run(
        Command::new("cargo")
            .args(["vendor", "--locked"])
            .current_dir(&crate_dir)
            // cargo vendor prints a config snippet on stdout we don't need.
            .stdout(Stdio::null()),
        "cargo vendor",
    )?;
    run(
        Command::new("tar")
            .args(["-cJf", "../vendor.tar.xz", "vendor"])
            .current_dir(&crate_dir),
        "creating vendor.tar.xz",
    )?;

    // SRPM.
    run(
        Command::new("rpmbuild")
            .arg("-bs")
            .arg("--define")
            .arg(format!("_sourcedir {}", work.display()))
            .arg("--define")
            .arg(format!("_srcrpmdir {}", work.display()))
            .arg(&spec_path),
        "rpmbuild -bs",
    )?;
    let srpm = find_srpm(work)?;

    if dry_run {
        if cfg.update_local {
            println!(
                "  📦 Dry run: would update spec at {} to {new}",
                spec_src.display()
            );
        }
        println!("  📦 Dry run: built {}, not submitting...", ...);
        return Ok(());
    }

    let mut copr = Command::new("copr-cli");
    copr.arg("build");
    if !cfg.wait {
        copr.arg("--nowait");
    }
    copr.arg(&cfg.project).arg(&srpm);
    run(&mut copr, "copr-cli build")?;

    // Write the updated version back to the repo spec so it stays in sync.
    if cfg.update_local {
        write(&spec_src, spec.as_bytes())?;
    }

    println!(
        "  📦 Submitted {crate_name} {new} to COPR project {}",
        cfg.project
    );
    Ok(())
}

// -- Helpers --

fn msg(s: impl Into<String>) -> ReleaseError {
    ReleaseError::Message(s.into())
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| msg(format!("reading {}: {e}", path.display())))
}

fn write(path: &Path, contents: &[u8]) -> Result<()> {
    fs::write(path, contents).map_err(|e| msg(format!("writing {}: {e}", path.display())))
}

fn regex(pattern: &str) -> Result<Regex> {
    Regex::new(pattern).map_err(|e| msg(format!("invalid regex {pattern:?}: {e}")))
}

fn run(cmd: &mut Command, what: &str) -> Result<()> {
    let status = cmd
        .status()
        .map_err(|e| msg(format!("{what}: failed to spawn: {e}")))?;
    if !status.success() {
        return Err(msg(format!("{what} failed ({status})")));
    }
    Ok(())
}

fn find_srpm(dir: &Path) -> Result<PathBuf> {
    fs::read_dir(dir)
        .map_err(|e| msg(format!("reading {}: {e}", dir.display())))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".src.rpm"))
        })
        .ok_or_else(|| msg("rpmbuild didn't produce a .src.rpm"))
}

// -- Pure transformations (unit-tested) --

/// The crate name from `%global crate <name>`.
fn spec_crate_name(spec: &str) -> Result<&str> {
    regex(r"(?m)^%global[ \t]+crate[ \t]+(\S+)[ \t]*$")?
        .captures(spec)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or_else(|| msg("no `%global crate <name>` line in spec"))
}

/// The spec must list `vendor.tar.xz` as a source, or the SRPM won't carry
/// the vendored dependencies and the COPR build will fail offline.
fn check_vendor_source(spec: &str) -> Result<()> {
    if regex(r"(?m)^Source\d*:[ \t]*vendor\.tar\.xz[ \t]*$")?.is_match(spec) {
        Ok(())
    } else {
        Err(msg("spec has no `SourceN: vendor.tar.xz` line"))
    }
}

/// Set the `Version:` field, keeping the original alignment.
fn set_spec_version(spec: &str, new: &str) -> Result<String> {
    let re = regex(r"(?m)^(Version:[ \t]*)\S+[ \t]*$")?;
    if !re.is_match(spec) {
        return Err(msg("no `Version:` line in spec"));
    }
    Ok(re
        .replace(spec, |c: &Captures<'_>| format!("{}{new}", &c[1]))
        .into_owned())
}

/// Pre-flight checks: tools, the spec, rpm macros, and COPR authentication.
#[must_use]
pub fn preflight(cfg: &CoprConfig, root: &Path) -> Vec<Check> {
    vec![
        (
            "COPR tools",
            require_tools(&["curl", "tar", "xz", "cargo", "rpmbuild", "copr-cli"]),
        ),
        ("COPR spec", check_spec(cfg, root)),
        ("COPR rpm macros", check_rpm_macros()),
        ("COPR auth", check_auth()),
    ]
}

fn check_spec(cfg: &CoprConfig, root: &Path) -> std::result::Result<String, String> {
    let spec = read(&root.join(&cfg.spec)).map_err(|e| e.to_string())?;
    let name = spec_crate_name(&spec).map_err(|e| e.to_string())?;
    check_vendor_source(&spec).map_err(|e| e.to_string())?;
    // Only checking that a Version: line exists; the result is discarded.
    set_spec_version(&spec, "0.0.0").map_err(|e| e.to_string())?;
    Ok(format!("{} (crate {name})", cfg.spec.display()))
}

/// `%{crates_source}` comes from cargo-rpm-macros; without it rpmbuild
/// can't resolve Source0.
fn check_rpm_macros() -> std::result::Result<String, String> {
    let output = Command::new("rpm")
        .args(["--eval", "%{?crates_source:yes}"])
        .output()
        .map_err(|e| format!("spawning rpm: {e}"))?;
    if String::from_utf8_lossy(&output.stdout).trim() == "yes" {
        Ok("cargo-rpm-macros available".to_owned())
    } else {
        Err("cargo-rpm-macros not installed (sudo dnf install cargo-rpm-macros)".to_owned())
    }
}

/// `copr-cli whoami` verifies the API token in ~/.config/copr.
fn check_auth() -> std::result::Result<String, String> {
    let output = Command::new("copr-cli")
        .arg("whoami")
        .output()
        .map_err(|e| format!("spawning copr-cli: {e}"))?;
    if output.status.success() {
        let user = String::from_utf8_lossy(&output.stdout);
        Ok(format!("authenticated as {}", user.trim()))
    } else {
        Err(format!(
            "copr-cli not authenticated (see copr.fedorainfracloud.org/api): {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "# Generated by rust2rpm 28
%bcond check 1

%global crate jj-release

Name:           rust-jj-release
Version:        0.8.0
Release:        %autorelease
Summary:        Semantic releases for Jujutsu VCS repositories

Source0:        %{crates_source}
Source1:        vendor.tar.xz
";

    #[test]
    fn reads_crate_name() {
        assert_eq!(spec_crate_name(SPEC).unwrap(), "jj-release");
    }

    #[test]
    fn errors_without_crate_name() {
        assert!(spec_crate_name("Name: foo\nVersion: 1.0.0\n").is_err());
    }

    #[test]
    fn finds_vendor_source() {
        assert!(check_vendor_source(SPEC).is_ok());
        assert!(check_vendor_source("Source0: %{crates_source}\n").is_err());
    }

    #[test]
    fn sets_version_keeping_alignment() {
        let out = set_spec_version(SPEC, "0.9.0").unwrap();
        assert!(out.contains("\nVersion:        0.9.0\n"));
        assert!(!out.contains("0.8.0"));
        // Release and everything else untouched.
        assert!(out.contains("\nRelease:        %autorelease\n"));
    }

    #[test]
    fn errors_without_version() {
        assert!(set_spec_version("Name: foo\n", "1.0.0").is_err());
    }

    #[test]
    fn update_local_writes_version_back_to_spec() {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("rust-jj-release.spec");
        fs::write(&spec_path, SPEC).unwrap();

        // Simulate what publish does, set the version and write back.
        let new_version = "0.9.0";
        let updated = set_spec_version(SPEC, new_version).unwrap();
        fs::write(&spec_path, updated.as_bytes()).unwrap();

        let content = fs::read_to_string(&spec_path).unwrap();
        assert!(content.contains("\nVersion:        0.9.0\n"));
        assert!(!content.contains("0.8.0"));
        // Other fields untouched.
        assert!(content.contains("%global crate jj-release"));
        assert!(content.contains("Release:        %autorelease"));
    }
}
