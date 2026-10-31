use std::collections::HashMap;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[non_exhaustive]
#[allow(clippy::struct_excessive_bools)]
pub struct ChangelogConfig {
    pub enabled: bool,
    pub file: String,
    /// If true, silently drops commits that don't pass strict Conventional Commits parsing.
    /// If false, unformatted commits are placed in an "Other Changes" section.
    pub strict: bool,
    pub prefix_mapping: HashMap<String, String>,
    pub exclude_prefixes: Vec<String>,
    pub emoji_headers: bool,
    pub emoji_mapping: HashMap<String, String>,
    pub link_headers: bool,
    pub layout: ChangelogLayout,
}

impl ChangelogConfig {
    #[must_use]
    pub const fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
    #[must_use]
    pub const fn emoji_headers(mut self, on: bool) -> Self {
        self.emoji_headers = on;
        self
    }
    #[must_use]
    pub fn map_prefix(mut self, prefix: &str, header: &str) -> Self {
        self.prefix_mapping
            .insert(prefix.to_owned(), header.to_owned());
        self
    }
    #[must_use]
    pub fn exclude_prefix(mut self, prefix: &str) -> Self {
        self.exclude_prefixes.push(prefix.to_owned());
        self
    }
}

impl Default for ChangelogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file: "CHANGELOG.md".to_owned(),
            strict: true,
            prefix_mapping: HashMap::new(),
            exclude_prefixes: vec![],
            emoji_headers: false,
            emoji_mapping: HashMap::new(),
            link_headers: false,
            layout: ChangelogLayout::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum ChangelogLayout {
    /// One changelog at the repository root.
    #[default]
    Root,
    /// One changelog in each workspace member's directory.
    PerMember,
}

