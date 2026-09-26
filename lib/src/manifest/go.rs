//! Go backend. Go modules are versioned by tag only, so there is no
//! manifest version to read or write.

use std::path::Path;

use semver::Version;

use super::ManifestBackend;
use crate::errors::Result;

pub struct GoManifest;

impl ManifestBackend for GoManifest {
    fn read_version(&self, _root: &Path) -> Result<Version> {
        // Return 0.0.0 so the bump logic computes from scratch.
        Ok(Version::new(0, 0, 0))
    }

    fn write_version(&self, _root: &Path, _version: &Version) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        let v = GoManifest.read_version(dir.path()).unwrap();
        assert_eq!(v, Version::new(0, 0, 0));
    }

    #[test]
    fn write_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        GoManifest
            .write_version(dir.path(), &Version::parse("1.0.0").unwrap())
            .unwrap();
        // No files created.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
