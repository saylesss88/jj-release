use semver::Version;

/// A commit message plus optional VCS metadata (jj change ID, git hash, etc.).
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub change_id: String,
    pub description: String,
}

impl CommitInfo {
    /// A commit with just a message. `change_id` is optional metadata
    /// (a jj change ID, git hash, etc.) and isn't used for rendering.
    #[must_use]
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            change_id: String::new(),
            description: description.into(),
        }
    }
}

impl From<&str> for CommitInfo {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// One release in a changelog.
#[derive(Debug, Clone, Default)]
pub struct Release {
    /// Workspace member the release belongs to, shown in its heading.
    pub name: Option<String>,
    /// `None` renders `[Unreleased]`.
    pub version: Option<Version>,
    /// `YYYY-MM-DD`. `None` uses today's date (ignored for unreleased).
    pub date: Option<String>,
    /// Link for the version heading, e.g. a forge compare URL.
    pub compare_url: Option<String>,
    pub commits: Vec<CommitInfo>,
}
