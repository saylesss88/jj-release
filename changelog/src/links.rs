use semver::Version;

/// A forge repository that release compare links point at.
#[derive(Debug, Clone)]
pub struct ForgeRepo {
    /// Forge base URL, e.g. `https://github.com`.
    pub base: String,
    /// `owner/repo`.
    pub repo: String,
}

impl ForgeRepo {
    /// A link comparing `prev` with `next`, or with the tip when `next` is `None`.
    #[must_use]
    pub fn compare_link(&self, prev: &Version, next: Option<&Version>, prefix: &str) -> String {
        next.map_or_else(
            || compare_unreleased_url(&self.base, &self.repo, prev, prefix),
            |next| compare_url(&self.base, &self.repo, prev, next, prefix),
        )
        // match next {
        //     Some(next) => compare_url(&self.base, &self.repo, prev, next, prefix),
        //     None => compare_unreleased_url(&self.base, &self.repo, prev, prefix),
        // }
    }
}

#[must_use]
pub fn compare_url(
    base_url: &str,
    owner_repo: &str,
    prev: &Version,
    next: &Version,
    tag_prefix: &str,
) -> String {
    format!("{base_url}/{owner_repo}/compare/{tag_prefix}{prev}...{tag_prefix}{next}")
}

#[must_use]
pub fn compare_unreleased_url(
    base_url: &str,
    owner_repo: &str,
    prev: &Version,
    tag_prefix: &str,
) -> String {
    format!("{base_url}/{owner_repo}/compare/{tag_prefix}{prev}...HEAD")
}

#[must_use]
pub fn forge_base_url(forge: &str, forge_url: &str) -> Option<String> {
    if !forge_url.is_empty() {
        return Some(forge_url.to_owned());
    }
    match forge {
        "github" => Some("https://github.com".to_owned()),
        "gitlab" => Some("https://gitlab.com".to_owned()),
        "forgejo" | "codeberg" => Some("https://codeberg.org".to_owned()),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_url_github() {
        let prev = Version::parse("0.8.0").unwrap();
        let next = Version::parse("0.9.0").unwrap();
        assert_eq!(
            compare_url(
                "https://github.com",
                "saylesss88/jj-release",
                &prev,
                &next,
                "v"
            ),
            "https://github.com/saylesss88/jj-release/compare/v0.8.0...v0.9.0"
        );
    }

    #[test]
    fn generates_correct_unreleased_compare_url() {
        let prev = Version::parse("0.10.0").unwrap();

        let url = compare_unreleased_url("https://github.com", "saylesss88/jj-release", &prev, "v");

        assert_eq!(
            url,
            "https://github.com/saylesss88/jj-release/compare/v0.10.0...HEAD"
        );
    }
}
