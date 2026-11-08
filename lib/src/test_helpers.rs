#[cfg(test)]
pub mod mock {
    use std::cell::RefCell;

    use crate::errors::Result;
    use crate::{commits::CommitInfo, jj::JjBackend};

    #[derive(Default)]
    pub struct MockBackend {
        pub tags: Vec<String>,
        pub commits: Vec<CommitInfo>,
        pub calls: RefCell<Vec<String>>,
    }

    impl JjBackend for MockBackend {
        fn list_tags(&self, revset: &str) -> Result<Vec<String>> {
            // Version baselines must stay on the current branch's ancestry. The one
            // exception is pre-release numbering (`apply_channel`), which has to see
            // every tag so it never reuses an existing `-rc.N`.
            assert!(
                matches!(revset, "tags() & ::@" | "tags()"),
                "Release pipeline must restrict tag searches to the current branch ancestry \
     (or to all tags, for pre-release numbering); got {revset:?}"
            );
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
        fn commit_date(&self, _rev: &str) -> Result<String> {
            Ok("2026-01-01".to_owned())
        }
        fn tag_exists(&self, tag: &str) -> Result<bool> {
            Ok(self.tags.contains(&tag.to_owned()))
        }
        fn current_operation(&self) -> Result<String> {
            Ok("0123456789ab".to_owned())
        }
    }
}
