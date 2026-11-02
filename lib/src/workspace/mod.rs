//! Workspace support for multi-crate projects.

pub mod bumps;
pub mod detect;
pub mod members;

pub use crate::manifest::WorkspaceManifest;
pub use bumps::{
    bump_member_version, detect_tag_prefix, member_bumps, member_fileset, member_since,
    member_tag_name,
};
pub use detect::{DetectedWorkspace, detect_workspace};
pub use members::{member_versions, ordered_members, parse_workspace_members};
