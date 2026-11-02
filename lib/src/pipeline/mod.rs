//! Release pipeline orchestration.

use std::collections::HashMap;

use semver::Version;

use jj_release_changelog::CommitInfo;

use crate::commits::BumpKind;
use crate::forge::ForgeBackend;
use crate::jj::JjBackend;
use crate::manifest::ManifestBackend;
use crate::publish::PublishBackend;

pub use changelog::{ChangelogOutput, print_changelog, print_full_changelog};
pub use next_version::print_next_version;
pub use prepare::{independent_workspace, prepare_release};
pub use release::{
    run_distro, run_preflight_checks, run_publish, update_changelog, update_workspace_dependencies,
};
pub use validate::validate;

mod changelog;
mod next_version;
mod prepare;
mod release;
mod validate;

/// A release that's ready to run
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PreparedRelease {
    /// Revset the release range starts from (a tag name or `root()`).
    pub since: String,
    /// Every commit in `since..@`, across the whole repository.
    pub commits: Vec<CommitInfo>,
    /// What gets versioned and tagged.
    pub plan: ReleasePlan,
}

/// How a release is versioned.
#[derive(Debug, Clone)]
pub enum ReleasePlan {
    /// One shared version and tag: a single crate or a unified workspace.
    Unified(UnifiedPlan),
    /// Per-member versions in an independent workspace: name -> (current, next).
    /// Members whose current and next versions are equal aren't released.
    Independent(HashMap<String, MemberPlan>),
}

/// Version details for a unified release.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UnifiedPlan {
    pub current_version: Version,
    pub baseline_version: Version,
    pub next_version: Version,
    pub tag_name: String,
    pub bump: BumpKind,
}

impl PreparedRelease {
    /// The unified plan, or `None` for an independent release.
    #[must_use]
    pub const fn unified(&self) -> Option<&UnifiedPlan> {
        match &self.plan {
            ReleasePlan::Unified(plan) => Some(plan),
            ReleasePlan::Independent(_) => None,
        }
    }

    /// Per-member `(current, next)` versions, or `None` for a unified release.
    #[must_use]
    pub const fn members(&self) -> Option<&HashMap<String, MemberPlan>> {
        match &self.plan {
            ReleasePlan::Independent(members) => Some(members),
            ReleasePlan::Unified(_) => None,
        }
    }

    /// Whether this is an independent workspace release.
    #[must_use]
    pub const fn is_independent(&self) -> bool {
        matches!(self.plan, ReleasePlan::Independent(_))
    }
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

/// Version plan for one member of an independent workspace.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MemberPlan {
    pub current: Version,
    pub next: Version,
    /// No tag exists for this member yet; `next == current` but it is still released.
    pub first_release: bool,
    /// Released only because a dependency's new version falls outside its
    /// requirement, not because of its own commits.
    pub propagated: bool,
    /// Workspace dependencies whose requirements change in this release.
    pub updated_deps: Vec<String>,
}

impl MemberPlan {
    /// Whether this member is released in this run.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.first_release || self.current != self.next
    }
}
