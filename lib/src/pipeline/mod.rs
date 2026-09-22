//! Release pipeline orchestration.

use std::collections::HashMap;

use semver::Version;

use crate::commits::{BumpKind, CommitInfo};
use crate::forge::ForgeBackend;
use crate::jj::JjBackend;
use crate::manifest::ManifestBackend;
use crate::publish::PublishBackend;

pub use changelog::{print_changelog, print_full_changelog};
pub use next_version::print_next_version;
pub use prepare::prepare_release;
pub use validate::validate;

mod changelog;
mod next_version;
mod prepare;
mod validate;

#[non_exhaustive]
pub struct PreparedRelease {
    pub since: String,
    pub current_version: Version,
    pub baseline_version: Version,
    pub next_version: Version,
    pub tag_name: String,
    pub commits: Vec<CommitInfo>,
    pub bump: BumpKind,
    pub member_bumps: Option<HashMap<String, (Version, Version)>>,
}

#[non_exhaustive]
pub struct ReleaseContext<'a> {
    pub backend: &'a dyn JjBackend,
    pub manifest: &'a dyn ManifestBackend,
    pub forge: &'a dyn ForgeBackend,
    pub publisher: &'a dyn PublishBackend,
}

impl<'a> ReleaseContext<'a> {
    pub fn new(
        backend: &'a dyn JjBackend,
        manifest: &'a dyn ManifestBackend,
        forge: &'a dyn ForgeBackend,
        publisher: &'a dyn PublishBackend,
    ) -> Self {
        Self {
            backend,
            manifest,
            forge,
            publisher,
        }
    }
}
