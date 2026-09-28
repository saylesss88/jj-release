# How it works

Add a trigger commit when you're ready to ship:

```sh
jj new -m "Release: please"
jj git push --bookmark main
```

CI detects the trigger, walks commits back to the last version tag, and
classifies them as patch/minor/major using
[Conventional Commits](https://www.conventionalcommits.org). API-breaking
changes are detected via
[cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks) and can
automatically upgrade the bump to major. The version baseline comes from
crates.io rather than the local manifest, ensuring the correct bump even when
versions have drifted.

A trigger is recognized when any commit reachable from `release.bookmark` has a
description containing `release.trigger` (case-sensitive substring match) since
the last version tag. The trigger commit itself is not included in changelog
generation or bump calculation unless its message also matches a Conventional
Commit type.

Accidentally triggering a release with `docs: Release: please improve wording`
is possible, choose a trigger string unlikely to appear in normal commit
messages.

### What a Release Changes

Unless `--dry-run` is used, `jj-release` may:

- Modify version manifests and `CHANGELOG.md`
- Create a release commit and version tag
- Move and push the configured bookmark
- Publish packages to the configured registry
- Create a GitHub, GitLab, or Forgejo release

Run `jj-release validate` and `jj-release --dry-run` before enabling it in CI.
