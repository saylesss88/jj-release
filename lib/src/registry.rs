//! Cargo registry checks.

use std::{
    env, fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use semver::Version;

use crate::errors::{ReleaseError, Result};

/// crates.io asks API clients to identify themselves.
const USER_AGENT: &str = "jj-release (https://github.com/saylesss88/jj-release)";

/// Whether exactly `version` of `name` is on crates.io, yanked or not.
///
/// Reads the sparse index, the same source `cargo publish` waits on. Unlike
/// `cargo search`, it lists every version, pre-releases included, and is
/// updated as soon as a publish lands.
///
/// # Errors
/// Returns an error if curl can't be run, or the index answers with
/// anything other than 200 or 404.
pub fn version_exists_on_crates_io(name: &str, version: &Version) -> Result<bool> {
    let url = format!("https://index.crates.io/{}", index_path(name));
    let (status, body) = curl_get(&url, None)?;
    match status {
        200 => Ok(body
            .lines()
            .filter_map(index_version)
            .any(|v| v == *version)),
        404 => Ok(false), // never published
        other => Err(ReleaseError::Message(format!(
            "crates.io index returned HTTP {other} for {name}"
        ))),
    }
}

/// Fetches the latest published version of a crate using local `cargo search`
///
/// # Errors
/// Returns an error if the crate doesn't exist or `cargo search` fails
pub fn latest_version_on_crates_io(name: &str) -> Result<Option<Version>> {
    let output = Command::new("cargo")
        .args(["search", name, "--limit", "1"])
        .output()
        .map_err(|e| ReleaseError::Message(format!("failed to execute cargo search: {e}")))?;

    if !output.status.success() {
        return Err(ReleaseError::Message(format!(
            "cargo search failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let exact_prefix = format!("{name} = \"");

    for line in stdout.lines() {
        if line.starts_with(&exact_prefix)
            && let Some(version_str) = line.split('"').nth(1)
        {
            return Ok(Version::parse(version_str).ok());
        }
    }

    // If the loop finishes without matching the exact prefix, the crate doesn't exist
    Ok(None)
}

/// What crates.io makes of the publish token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenStatus {
    /// crates.io accepted the token.
    Accepted,
    /// crates.io rejected the token: expired, revoked, or mistyped.
    Rejected,
    /// The token couldn't be checked: there's none jj-release can read (it
    /// may be held by a credential provider), or crates.io answered in an
    /// unexpected way.
    Unknown(String),
}

/// Ask crates.io whether it accepts the token `cargo publish` would use.
///
/// `/api/v1/me` only accepts browser sessions, so this calls a search route
/// that requires authentication instead. A scoped publish token is refused
/// there with "does not have the required permissions", which still means
/// crates.io recognised it; an unusable one fails with "authentication failed".
///
/// # Errors
/// Returns an error if curl can't be run.
pub fn token_status() -> Result<TokenStatus> {
    let Some(token) = publish_token() else {
        return Ok(TokenStatus::Unknown(
            "no token in CARGO_REGISTRY_TOKEN or Cargo's credentials file".to_owned(),
        ));
    };

    let (status, body) = curl_get(
        "https://crates.io/api/v1/crates?following=1&per_page=1",
        Some(&format!("Authorization: {token}")),
    )?;

    Ok(match status {
        200 => TokenStatus::Accepted,
        403 if body.contains("required permissions") => TokenStatus::Accepted,
        401 | 403 if body.contains("authentication failed") => TokenStatus::Rejected,
        other => TokenStatus::Unknown(format!("crates.io returned HTTP {other}")),
    })
}

/// The crates.io token cargo would use, if it's one jj-release can read:
/// `CARGO_REGISTRY_TOKEN`, or `[registry] token` in Cargo's credentials file.
fn publish_token() -> Option<String> {
    if let Ok(token) = env::var("CARGO_REGISTRY_TOKEN")
        && !token.is_empty()
    {
        return Some(token);
    }

    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".cargo")))?;

    // Cargo reads `credentials.toml`, or the older `credentials`.
    ["credentials.toml", "credentials"].iter().find_map(|file| {
        let content = fs::read_to_string(cargo_home.join(file)).ok()?;
        let table: toml::Table = content.parse().ok()?;
        table
            .get("registry")?
            .get("token")?
            .as_str()
            .map(ToOwned::to_owned)
    })
}

/// A crate's path in the sparse index, e.g. `jj/-r/jj-release`.
fn index_path(name: &str) -> String {
    let name = name.to_lowercase();
    match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    }
}

/// The `"vers"` field of one sparse-index line (one JSON object per version).
fn index_version(line: &str) -> Option<Version> {
    let rest = line.split_once(r#""vers":""#)?.1;
    Version::parse(rest.split('"').next()?).ok()
}

/// GET `url` with curl, returning the HTTP status and body. A `header` goes
/// to curl on stdin (`--header @-`), so a token never appears in arguments.
fn curl_get(url: &str, header: Option<&str>) -> Result<(u16, String)> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--location",
        "--user-agent",
        USER_AGENT,
        "--write-out",
        "\n%{http_code}",
    ]);
    if header.is_some() {
        cmd.args(["--header", "@-"]);
    }
    cmd.arg(url)
        .stdin(if header.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| ReleaseError::Message(format!("failed to run curl: {e}")))?;
    if let (Some(header), Some(mut stdin)) = (header, child.stdin.take()) {
        writeln!(stdin, "{header}")?;
        // Dropping stdin closes it, so curl stops reading headers.
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(ReleaseError::Message(format!(
            "curl {url} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (body, status) = stdout
        .rsplit_once('\n')
        .ok_or_else(|| ReleaseError::Message(format!("unexpected curl output for {url}")))?;
    let status = status
        .trim()
        .parse()
        .map_err(|_| ReleaseError::Message(format!("unexpected HTTP status from {url}")))?;
    Ok((status, body.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_paths_follow_cargo_layout() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("jj-release"), "jj/-r/jj-release");
        assert_eq!(index_path("Serde"), "se/rd/serde");
    }

    #[test]
    fn reads_version_from_index_line() {
        let line = r#"{"name":"jj-release","vers":"0.11.0-rc.1","deps":[],"cksum":"ab","features":{},"yanked":false}"#;
        assert_eq!(
            index_version(line),
            Some(Version::parse("0.11.0-rc.1").unwrap())
        );
        assert_eq!(index_version("{}"), None);
    }
    #[test]
    #[ignore = "Makes HTTP request"]
    fn published_version_exists() {
        let v = Version::parse("1.0.0").unwrap();
        assert!(version_exists_on_crates_io("thiserror", &v).unwrap());
    }

    #[test]
    #[ignore = "Makes HTTP request"]
    fn unpublished_version_does_not_exist() {
        let v = Version::parse("999.0.0").unwrap();
        assert!(!version_exists_on_crates_io("thiserror", &v).unwrap());
    }

    #[test]
    #[ignore = "Makes HTTP request"]
    fn gets_latest_version_from_crates_io() {
        let v = latest_version_on_crates_io("thiserror").unwrap();
        assert!(v.is_some());
        assert!(v.unwrap().major >= 1);
    }
}
