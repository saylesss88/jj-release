//! Publishing new versions to distribution package repositories.
//!
//! Each target is behind its own Cargo feature:
//! - `publish-aur`  -> [`aur`]
//! - `publish-copr` -> [`copr`]

#[cfg(feature = "publish-aur")]
pub mod aur;
#[cfg(feature = "publish-copr")]
pub mod copr;

/// Download `url` with curl, retrying for about a minute. crates.io's static
/// CDN can take a few seconds to serve a crate after `cargo publish`.
#[cfg(any(feature = "publish-aur", feature = "publish-copr"))]
fn download(url: &str) -> crate::errors::Result<Vec<u8>> {
    use std::process::Command;

    use crate::errors::ReleaseError;

    let output = Command::new("curl")
        .args([
            "-sSLf",
            "--retry",
            "6",
            "--retry-delay",
            "10",
            "--retry-all-errors",
            url,
        ])
        .output()
        .map_err(|e| ReleaseError::Message(format!("spawning curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ReleaseError::Message(format!(
            "downloading {url} failed: {}",
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}
