//! Semantic releases for Jujutsu VCS repositories.
//!
//! # Overview
//! jj-release automates versioning, changelog generation, tagging,
//! and publishing for projects using the [Jujutsu](https://github.com/jj-vcs/jj)
//! version control system.
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use jj_release::pipeline::{prepare_release, ReleaseContext};
//! use jj_release::manifest::CargoManifest;
//! use jj_release::config::Config;
//! use jj_release::jj::ShellBackend;
//!
//! let root = Path::new(".");
//! let backend = ShellBackend::new(root).unwrap();
//! let manifest = CargoManifest;
//! let config = Config::default();
//!
//! if let Some(release) = prepare_release(&backend, &manifest, &config, root).unwrap() {
//!     println!("Next version: {}", release.next_version);
//! }
//! ```

pub mod changelog;
pub mod commits;
pub mod config;
pub mod detect;
pub mod errors;
pub mod forge;
pub mod jj;
pub mod manifest;
pub mod pipeline;
pub mod publish;
pub mod registry;
pub mod workspace;

#[cfg(test)]
pub mod test_helpers;

pub use commits::{BumpKind, CommitInfo, Tag};
pub use config::Config;
pub use forge::ForgeBackend;
pub use jj::JjBackend;
pub use manifest::ManifestBackend;
pub use pipeline::{PreparedRelease, ReleaseContext};
pub use publish::PublishBackend;
