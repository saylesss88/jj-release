use std::path::PathBuf;

pub struct WorkspaceMember {
    pub name: String,
    pub path: PathBuf,
    pub publish: bool,
    pub depends_on: Vec<String>, // other member names
}

pub struct WorkspaceConfig {
    pub versioning: Versioning,
    pub members: Vec<WorkspaceMember>,
}

pub enum Versioning {
    Unified,     // one version in [workspace.package]
    Independent, // each member has its own version
}

pub struct WorkspaceManifest {
    pub config: WorkspaceConfig,
}
