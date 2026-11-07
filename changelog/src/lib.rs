#![doc = include_str!("../README.md")]

mod config;
mod links;
mod model;
mod prepend;
mod render;

pub use config::{ChangelogConfig, ChangelogLayout};
pub use links::{ForgeRepo, compare_unreleased_url, compare_url, forge_base_url};
pub use model::{CommitInfo, Release};
pub use prepend::{has_changelog_title, prepend_to_file};
pub use render::{
    render_body, render_changelog, render_changelog_section, render_heading,
    render_pending_section, render_release,
};
