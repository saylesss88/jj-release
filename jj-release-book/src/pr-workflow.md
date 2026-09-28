# Release PRs

If you want to review before it publishes, use the `pr` subcommand instead:

```sh
jj new -m "Release: please"
jj-release pr
```

This pushes your current bookmark to a `release/vX.Y.Z` branch and opens a PR
with the changelog preview as the PR body. No version bump, no release commit,
no tag. Merge the PR and CI runs `jj-release` to do the actual release.

<!-- prettier-ignore -->
> [!NOTE]
> Unlike `release-plz`'s continuously maintained release PR, `jj-release pr` is a
> one-shot preview. No version bump or release commit in the PR itself. If you
> want a persistent release PR that stays up to date, run `jj-release pr` in CI
> on every push to main.
