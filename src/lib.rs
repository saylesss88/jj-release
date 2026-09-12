//! Semantic releases for Jujutsu VCS repositories.
//!
//! # Overview
//! jj-release automates versioning, changelog generation, tagging,
//! and publishing for projects using the [Jujutsu](https://github.com/jj-vcs/jj)
//! version control system.
//!
//! # Example
//! ```no_run
//! use jj_release::config::Config;
//! use jj_release::pipeline::prepare_release;
//! ```

pub mod changelog;
pub mod commits;
pub mod config;
pub mod forge;
pub mod jj;
pub mod manifest;
pub mod pipeline;
pub mod publish;
pub mod workspace;

pub use commits::{BumpKind, CommitInfo, Tag};
pub use config::Config;
pub use forge::ForgeBackend;
pub use jj::JjBackend;
pub use manifest::ManifestBackend;
pub use pipeline::{PreparedRelease, ReleaseContext};
pub use publish::PublishBackend;
