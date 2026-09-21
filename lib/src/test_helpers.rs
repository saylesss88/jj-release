#[cfg(test)]
pub mod mock {
    use std::cell::RefCell;

    use anyhow::Result;

    use crate::{commits::CommitInfo, jj::JjBackend};

    #[derive(Default)]
    pub struct MockBackend {
        pub tags: Vec<String>,
        pub commits: Vec<CommitInfo>,
        pub calls: RefCell<Vec<String>>,
    }

    impl JjBackend for MockBackend {
        fn list_tags(&self) -> Result<Vec<String>> {
            Ok(self.tags.clone())
        }
        fn log_commits(&self, revset: &str) -> Result<Vec<CommitInfo>> {
            self.calls.borrow_mut().push(revset.to_owned());
            Ok(self.commits.clone())
        }
        fn log_commits_for_path(&self, revset: &str, _path: &str) -> Result<Vec<CommitInfo>> {
            self.calls.borrow_mut().push(revset.to_owned());
            Ok(self.commits.clone())
        }
        fn new_commit(&self, _: &str) -> Result<String> {
            Ok(String::new())
        }
        fn create_tag(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn set_bookmark(&self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
        fn git_push(&self, _: Option<&str>, _: Option<&str>) -> Result<()> {
            Ok(())
        }
        fn git_export(&self) -> Result<()> {
            Ok(())
        }
        fn check_identity(&self) -> Result<()> {
            Ok(())
        }
    }
}
