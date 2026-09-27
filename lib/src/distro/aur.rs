//! Publish a new version to the AUR.
//!
//! Clones the package's AUR repo, bumps `pkgver` and resets `pkgrel`,
//! downloads the new remote sources to recompute their checksums, updates
//! `PKGBUILD` and `.SRCINFO`, then commits and pushes.
//!
//! `.SRCINFO` is regenerated with `makepkg --printsrcinfo` when `makepkg` is
//! available, and edited in place otherwise (only version-dependent fields
//! change on a bump, so targeted edits are reliable).
//!
//! Supported: a single package with `sha256sums` and/or `b2sums`. Other
//! checksum arrays and architecture-specific sources are rejected with a
//! clear error. Non-HTTP sources (local files, VCS sources) keep their
//! existing checksums.

use std::{fmt::Write as _, fs, path::Path, process::Command};

use blake2::Blake2b512;
use regex::{NoExpand, Regex};
use semver::Version;
use sha2::{Digest, Sha256};

use super::download;
use crate::config::AurConfig;
use crate::errors::{ReleaseError, Result};

/// Checksum arrays makepkg supports that this module does not.
const UNSUPPORTED_SUMS: &[&str] = &[
    "md5sums",
    "sha1sums",
    "sha224sums",
    "sha384sums",
    "sha512sums",
    "cksums",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChecksumKind {
    Sha256,
    B2,
}

impl ChecksumKind {
    const ALL: [Self; 2] = [Self::Sha256, Self::B2];

    const fn key(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256sums",
            Self::B2 => "b2sums",
        }
    }

    fn hash(self, bytes: &[u8]) -> String {
        match self {
            Self::Sha256 => to_hex(&Sha256::digest(bytes)),
            Self::B2 => to_hex(&Blake2b512::digest(bytes)),
        }
    }
}

/// Bump the AUR package to `version`, then commit and push.
///
/// With `dry_run`, prints the staged diff and stops before committing.
///
/// # Errors
/// Returns an error if cloning, downloading a source, editing the package
/// files, or any git command fails, or if the package uses a checksum
/// array or source layout this module doesn't support.
pub fn publish(cfg: &AurConfig, version: &Version, dry_run: bool) -> Result<()> {
    let new = version.to_string();

    let tmp = tempfile::tempdir().map_err(|e| msg(format!("creating temp dir: {e}")))?;
    let dir = tmp.path().join(&cfg.package);

    let status = Command::new("git")
        .args(["clone", "--depth", "1", &cfg.repo_url()])
        .arg(&dir)
        .status()
        .map_err(|e| msg(format!("spawning git clone: {e}")))?;
    if !status.success() {
        return Err(msg(format!("git clone {} failed", cfg.repo_url())));
    }

    let pkgbuild_path = dir.join("PKGBUILD");
    let srcinfo_path = dir.join(".SRCINFO");
    let pkgbuild = read(&pkgbuild_path)?;
    let srcinfo = read(&srcinfo_path)?;

    let old = srcinfo_values(&srcinfo, "pkgver")
        .first()
        .copied()
        .ok_or_else(|| msg("no pkgver in .SRCINFO"))?
        .to_owned();
    if old == new {
        println!(
            "  📦 AUR package {} is already at {new}, skipping",
            cfg.package
        );
        return Ok(());
    }

    let kinds = detect_checksums(&srcinfo)?;
    let bumped = bump_srcinfo(&srcinfo, &old, &new);
    let sources = srcinfo_values(&bumped, "source");

    // Download each remote source once, reuse it for every checksum kind.
    let downloads: Vec<Option<Vec<u8>>> = sources
        .iter()
        .map(|s| source_url(s).map(download).transpose())
        .collect::<Result<_>>()?;

    let mut checksums = Vec::new();
    for kind in kinds {
        let old_sums = srcinfo_values(&srcinfo, kind.key());
        if old_sums.len() != sources.len() {
            return Err(msg(format!(
                "{} has {} entries but there are {} sources",
                kind.key(),
                old_sums.len(),
                sources.len()
            )));
        }
        let sums: Vec<String> = downloads
            .iter()
            .zip(&old_sums)
            .map(|(bytes, old_sum)| {
                bytes
                    .as_deref()
                    .map_or_else(|| (*old_sum).to_owned(), |b| kind.hash(b))
            })
            .collect();
        checksums.push((kind, sums));
    }

    write(&pkgbuild_path, &bump_pkgbuild(&pkgbuild, &new, &checksums)?)?;

    let new_srcinfo = makepkg_srcinfo(&dir).unwrap_or_else(|| {
        checksums.iter().fold(bumped.clone(), |s, (kind, sums)| {
            set_srcinfo_checksums(&s, *kind, sums)
        })
    });
    write(&srcinfo_path, &new_srcinfo)?;

    git(&dir, &["add", "PKGBUILD", ".SRCINFO"])?;

    if dry_run {
        git(&dir, &["--no-pager", "diff", "--cached"])?;
        println!("  📦 Dry run: not pushing {} {new} to the AUR", cfg.package);
        return Ok(());
    }

    git(&dir, &["commit", "-m", &format!("Update to {new}")])?;
    git(&dir, &["push"])?;

    println!("  📦 Pushed {} {new} to the AUR", cfg.package);
    Ok(())
}

// -- Helpers --

fn msg(s: impl Into<String>) -> ReleaseError {
    ReleaseError::Message(s.into())
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| msg(format!("reading {}: {e}", path.display())))
}

fn write(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).map_err(|e| msg(format!("writing {}: {e}", path.display())))
}

fn regex(pattern: &str) -> Result<Regex> {
    Regex::new(pattern).map_err(|e| msg(format!("invalid regex {pattern:?}: {e}")))
}

fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .map_err(|e| msg(format!("spawning git: {e}")))?;
    if !status.success() {
        return Err(msg(format!("git {} failed", args.join(" "))));
    }
    Ok(())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// `makepkg --printsrcinfo`, or `None` if makepkg isn't available or fails.
fn makepkg_srcinfo(dir: &Path) -> Option<String> {
    let out = Command::new("makepkg")
        .arg("--printsrcinfo")
        .current_dir(dir)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

// -- Pure transformations (unit-tested) --

/// All values for `key` in a .SRCINFO, in order.
fn srcinfo_values<'a>(srcinfo: &'a str, key: &str) -> Vec<&'a str> {
    srcinfo
        .lines()
        .filter_map(|l| l.trim().split_once(" = "))
        .filter(|(k, _)| *k == key)
        .map(|(_, v)| v)
        .collect()
}

/// Which supported checksum arrays the package uses. Errors on layouts
/// this module can't handle.
fn detect_checksums(srcinfo: &str) -> Result<Vec<ChecksumKind>> {
    let keys: Vec<&str> = srcinfo
        .lines()
        .filter_map(|l| l.trim().split_once(" = ").map(|(k, _)| k))
        .collect();

    for key in &keys {
        let (base, arch_specific) = key
            .split_once('_')
            .map_or((*key, false), |(base, _)| (base, true));
        if UNSUPPORTED_SUMS.contains(&base) {
            return Err(msg(format!(
                "unsupported checksum array `{key}` in .SRCINFO; use sha256sums or b2sums"
            )));
        }
        if arch_specific && matches!(base, "source" | "sha256sums" | "b2sums") {
            return Err(msg(format!(
                "architecture-specific `{key}` in .SRCINFO is not supported yet"
            )));
        }
    }

    let kinds: Vec<ChecksumKind> = ChecksumKind::ALL
        .into_iter()
        .filter(|kind| keys.contains(&kind.key()))
        .collect();
    if kinds.is_empty() {
        return Err(msg("no sha256sums or b2sums found in .SRCINFO"));
    }
    Ok(kinds)
}

/// The URL of a source entry (`name::url` or `url`), if it's downloadable
/// over HTTP. Local files and VCS sources return `None`.
fn source_url(entry: &str) -> Option<&str> {
    let url = entry.split_once("::").map_or(entry, |(_, url)| url);
    (url.starts_with("https://") || url.starts_with("http://")).then_some(url)
}

/// Set `pkgver`, reset `pkgrel`, and move `source` entries to the new version.
fn bump_srcinfo(srcinfo: &str, old: &str, new: &str) -> String {
    let mut out = String::with_capacity(srcinfo.len());
    for line in srcinfo.lines() {
        let indent = &line[..line.len() - line.trim_start().len()];
        match line.trim().split_once(" = ") {
            Some(("pkgver", _)) => {
                let _ = writeln!(out, "{indent}pkgver = {new}");
            }
            Some(("pkgrel", _)) => {
                let _ = writeln!(out, "{indent}pkgrel = 1");
            }
            Some(("source", value)) => {
                let _ = writeln!(out, "{indent}source = {}", value.replace(old, new));
            }
            _ => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Replace every `kind` line in a .SRCINFO with `sums`, in place of the first.
fn set_srcinfo_checksums(srcinfo: &str, kind: ChecksumKind, sums: &[String]) -> String {
    let mut out = String::with_capacity(srcinfo.len());
    let mut written = false;
    for line in srcinfo.lines() {
        if let Some((key, _)) = line.trim().split_once(" = ")
            && key == kind.key()
        {
            if !written {
                let indent = &line[..line.len() - line.trim_start().len()];
                for sum in sums {
                    let _ = writeln!(out, "{indent}{} = {sum}", kind.key());
                }
                written = true;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Format a Bash array the way PKGBUILDs usually align multi-line arrays.
fn bash_array(key: &str, values: &[String]) -> String {
    let pad = " ".repeat(key.len() + 2);
    let items: Vec<String> = values.iter().map(|v| format!("'{v}'")).collect();
    format!("{key}=({})", items.join(&format!("\n{pad}")))
}

/// Set `pkgver`, reset `pkgrel`, and replace checksum arrays in a PKGBUILD.
fn bump_pkgbuild(
    pkgbuild: &str,
    new: &str,
    checksums: &[(ChecksumKind, Vec<String>)],
) -> Result<String> {
    let pkgver = regex(r"(?m)^pkgver=.*$")?;
    let pkgrel = regex(r"(?m)^pkgrel=.*$")?;
    if !pkgver.is_match(pkgbuild) {
        return Err(msg("no pkgver= line in PKGBUILD"));
    }

    let mut out = pkgver
        .replace(pkgbuild, NoExpand(&format!("pkgver={new}")))
        .into_owned();
    out = pkgrel.replace(&out, NoExpand("pkgrel=1")).into_owned();

    for (kind, sums) in checksums {
        let array = regex(&format!(r"(?ms)^{}=\(.*?\)", kind.key()))?;
        if !array.is_match(&out) {
            return Err(msg(format!("no {}=(...) array in PKGBUILD", kind.key())));
        }
        let replacement = bash_array(kind.key(), sums);
        out = array.replace(&out, NoExpand(&replacement)).into_owned();
    }
    Ok(out)
}

/// Pre-flight checks: tools, the package's current layout, and SSH access.
#[must_use]
pub fn preflight(cfg: &AurConfig) -> Vec<Check> {
    let tools = require_tools(&["git", "curl"]).map(|found| {
        if tool_available("makepkg") {
            format!("{found}, makepkg (regenerates .SRCINFO)")
        } else {
            format!("{found} (no makepkg: .SRCINFO edited in place)")
        }
    });
    vec![
        ("AUR tools", tools),
        ("AUR package", check_package(cfg)),
        ("AUR SSH access", check_access(cfg)),
    ]
}

/// Fetch the current .SRCINFO over HTTPS and make sure we can handle it.
fn check_package(cfg: &AurConfig) -> std::result::Result<String, String> {
    if cfg.repo.is_some() {
        return Ok("custom repo, layout checked at publish time".to_owned());
    }
    let url = format!(
        "https://aur.archlinux.org/cgit/aur.git/plain/.SRCINFO?h={}",
        cfg.package
    );
    let bytes = fetch(&url).map_err(|e| format!("{} not found on the AUR ({e})", cfg.package))?;
    let srcinfo = String::from_utf8_lossy(&bytes);

    let version = srcinfo_values(&srcinfo, "pkgver")
        .first()
        .copied()
        .ok_or_else(|| "no pkgver in .SRCINFO".to_owned())?
        .to_owned();
    let kinds = detect_checksums(&srcinfo).map_err(|e| e.to_string())?;
    let sums: Vec<&str> = kinds.iter().map(|k| k.key()).collect();

    Ok(format!(
        "{} at {version}, {}",
        cfg.package,
        sums.join(" + ")
    ))
}

/// `git ls-remote` proves the SSH key is accepted without cloning anything.
fn check_access(cfg: &AurConfig) -> std::result::Result<String, String> {
    let url = cfg.repo_url();
    let output = Command::new("git")
        .args(["ls-remote", "--heads", &url])
        .output()
        .map_err(|e| format!("spawning git: {e}"))?;
    if output.status.success() {
        Ok(format!("can reach {url}"))
    } else {
        Err(format!(
            "cannot reach {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRCINFO: &str = "pkgbase = jj-release
\tpkgdesc = Semantic releases and changelog generation for jj-vcs repositories
\tpkgver = 0.8.0
\tpkgrel = 2
\turl = https://github.com/saylesss88/jj-release
\tarch = x86_64
\tsource = jj-release-0.8.0.tar.gz::https://static.crates.io/crates/jj-release/jj-release-0.8.0.crate
\tb2sums = oldhash

pkgname = jj-release
";

    const PKGBUILD: &str = "pkgname=jj-release
pkgver=0.8.0
pkgrel=2
source=(\"$pkgname-$pkgver.tar.gz::https://static.crates.io/crates/$pkgname/$pkgname-$pkgver.crate\")
b2sums=('oldhash')

build() {
  cargo build --release
}
";

    #[test]
    fn hashes_match_known_vectors() {
        assert_eq!(
            ChecksumKind::Sha256.hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            ChecksumKind::B2.hash(b"abc"),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }

    #[test]
    fn bumps_srcinfo_version_rel_and_source() {
        let out = bump_srcinfo(SRCINFO, "0.8.0", "0.9.0");
        assert!(out.contains("\tpkgver = 0.9.0\n"));
        assert!(out.contains("\tpkgrel = 1\n"));
        assert!(out.contains(
            "\tsource = jj-release-0.9.0.tar.gz::https://static.crates.io/crates/jj-release/jj-release-0.9.0.crate\n"
        ));
        // Untouched lines, blank separator and pkgname section survive.
        assert!(out.contains("\turl = https://github.com/saylesss88/jj-release\n"));
        assert!(out.contains("\n\npkgname = jj-release\n"));
    }

    #[test]
    fn sets_srcinfo_checksums() {
        let out = set_srcinfo_checksums(SRCINFO, ChecksumKind::B2, &["newhash".to_owned()]);
        assert!(out.contains("\tb2sums = newhash\n"));
        assert!(!out.contains("oldhash"));
    }

    #[test]
    fn bumps_pkgbuild() {
        let out = bump_pkgbuild(
            PKGBUILD,
            "0.9.0",
            &[(ChecksumKind::B2, vec!["newhash".to_owned()])],
        )
        .unwrap();
        assert!(out.contains("\npkgver=0.9.0\n"));
        assert!(out.contains("\npkgrel=1\n"));
        assert!(out.contains("\nb2sums=('newhash')\n"));
        // The source line uses $pkgver, so it must be left alone.
        assert!(out.contains("$pkgname-$pkgver.crate"));
        assert!(out.contains("cargo build --release"));
    }

    #[test]
    fn bumps_multiline_pkgbuild_array() {
        let pkgbuild = "pkgver=1.0.0\npkgrel=1\nsha256sums=('a'\n            'b')\n";
        let out = bump_pkgbuild(
            pkgbuild,
            "1.1.0",
            &[(ChecksumKind::Sha256, vec!["c".to_owned(), "d".to_owned()])],
        )
        .unwrap();
        assert!(out.contains("sha256sums=('c'\n            'd')\n"));
    }

    #[test]
    fn detects_b2sums() {
        assert_eq!(detect_checksums(SRCINFO).unwrap(), vec![ChecksumKind::B2]);
    }

    #[test]
    fn rejects_unsupported_checksums() {
        let srcinfo = "\tsource = https://example.com/a.tar.gz\n\tsha512sums = x\n";
        assert!(detect_checksums(srcinfo).is_err());
    }

    #[test]
    fn rejects_arch_specific_sources() {
        let srcinfo = "\tsource_x86_64 = https://example.com/a.tar.gz\n\tb2sums_x86_64 = x\n";
        assert!(detect_checksums(srcinfo).is_err());
    }

    #[test]
    fn parses_source_urls() {
        assert_eq!(
            source_url("name.tar.gz::https://example.com/a.crate"),
            Some("https://example.com/a.crate")
        );
        assert_eq!(
            source_url("https://example.com/a.tar.gz"),
            Some("https://example.com/a.tar.gz")
        );
        assert_eq!(source_url("jj-release.patch"), None);
        assert_eq!(source_url("git+https://example.com/repo.git"), None);
    }
}
